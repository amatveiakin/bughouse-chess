use std::collections::{hash_map, HashMap};
use std::hash::Hash;

use crate::session::*;


// Stores session data and updates subscribers on any changes.
// Multiple subscribers per session are possible because multiple client
// websockets can be connected with the same session id.
pub type SessionStore = Store<SessionId, Session>;

#[derive(Clone, Debug, Hash, Eq, PartialEq)]
pub struct SessionId(pub String);

impl SessionId {
    pub fn new(s: String) -> Self { Self(s) }
}

pub struct Store<K, V> {
    entries: HashMap<K, Entry<V>>,
    on_any_change: Option<Box<dyn Fn(&K, &V) + Send>>,
}

#[derive(Default, Hash, Eq, PartialEq, Clone, Copy)]
pub struct SubscriptionId(usize);

// Generic update-broadcasting map type.
impl<K, V> Store<K, V>
where
    K: Eq + PartialEq + Hash + Clone,
    V: Default,
{
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
            on_any_change: None,
        }
    }

    pub fn get(&self, id: &K) -> Option<&V> { self.entries.get(id).map(|e| &e.value) }

    // Sets the new Session data and notifies all subscribers.
    pub fn set(&mut self, id: K, value: V) {
        self.on_any_change.as_ref().map(|f| f(&id, &value));
        match self.entries.entry(id) {
            hash_map::Entry::Vacant(v) => {
                v.insert(Entry {
                    value,
                    subscriber_tx: HashMap::new(),
                    next_subscription_id: SubscriptionId(0),
                });
            }
            hash_map::Entry::Occupied(mut o) => o.get_mut().update(value),
        }
    }

    // Registers a subscriber and immediately calls it with the current
    // session. If there is no session, Session::default() is passed.
    pub fn subscribe(
        &mut self, id: &K, subscriber_tx: impl Fn(&V) + Send + 'static,
    ) -> SubscriptionId {
        self.entries.entry(id.clone()).or_default().subscribe(subscriber_tx)
    }

    pub fn unsubscribe(&mut self, id: &K, subscription_id: SubscriptionId) {
        self.entries.get_mut(id).map(|e| e.unsubscribe(subscription_id));
    }

    // Registers a callback for any change in the store.
    // The callback is NOT called for the existing values.
    // Only one callback can be registered at a time.
    pub fn on_any_change(&mut self, f: impl Fn(&K, &V) + Send + 'static) {
        assert!(
            self.on_any_change.is_none(),
            "Setting the on_any_change callback twice is not supported."
        );
        self.on_any_change = Some(Box::new(f));
    }

    // Runs the on_any_change callback, possibly creating a default entry.
    // Does not update subscribers.
    pub fn touch(&mut self, id: &K) {
        let v = self.entries.entry(id.clone()).or_default();
        self.on_any_change.as_ref().map(|f| f(&id, &v.value));
    }

    pub fn update_if_exists<F: FnOnce(&mut V)>(&mut self, id: &K, f: F) {
        if let Some(entry) = self.entries.get_mut(id) {
            f(&mut entry.value);
            self.on_any_change.as_ref().map(|f| f(&id, &entry.value));
            entry.update_subscribers();
        }
    }
}

#[derive(Default)]
struct Entry<V> {
    value: V,
    subscriber_tx: HashMap<SubscriptionId, Box<dyn Fn(&V) + Send>>,
    next_subscription_id: SubscriptionId,
}

impl<V> Entry<V> {
    fn update(&mut self, value: V) {
        self.value = value;
        self.update_subscribers();
    }
    fn update_subscribers(&mut self) {
        for subscriber_tx in self.subscriber_tx.values() {
            subscriber_tx(&self.value);
        }
    }
    fn subscribe(&mut self, subscriber_tx: impl Fn(&V) + Send + 'static) -> SubscriptionId {
        let subscription_id = self.next_subscription_id;
        self.next_subscription_id.0 += 1;
        subscriber_tx(&self.value);
        self.subscriber_tx.insert(subscription_id, Box::new(subscriber_tx));
        subscription_id
    }
    fn unsubscribe(&mut self, subscription_id: SubscriptionId) {
        self.subscriber_tx.remove(&subscription_id);
    }
}
