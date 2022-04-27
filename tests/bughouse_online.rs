// TODO: Mock clock.

use std::fmt;
use std::ops;
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use crossterm::{event as term_event};
use term_event::{KeyCode, KeyModifiers};

use bughouse_chess::*;


struct Server {
    clients: Arc<Mutex<server::Clients>>,
    state: server::ServerState,
}

impl Server {
    fn new() -> Self {
        let chess_rules = ChessRules {
            starting_position: StartingPosition::Classic,
            time_control: TimeControl{ starting_time: Duration::from_secs(300) },
        };
        let bughouse_rules = BughouseRules {
            min_pawn_drop_row: SubjectiveRow::from_one_based(2),
            max_pawn_drop_row: SubjectiveRow::from_one_based(6),
            drop_aggression: DropAggression::NoChessMate,
        };
        let clients = Arc::new(Mutex::new(server::Clients::new()));
        let clients_copy = Arc::clone(&clients);
        let state = server::ServerState::new(clients_copy, chess_rules, bughouse_rules);
        Server{ clients, state }
    }

    fn add_client(&mut self, events_tx: mpsc::Sender<BughouseServerEvent>) -> server::ClientId {
        self.clients.lock().unwrap().add_client(events_tx)
    }

    fn send_network_event(&mut self, id: server::ClientId, event: BughouseClientEvent) {
        self.state.apply_event(server::IncomingEvent::Network(id, event));
    }
    #[allow(dead_code)] fn tick(&mut self) {
        self.state.apply_event(server::IncomingEvent::Tick);
    }
}


#[derive(Debug)]
#[must_use]
struct ClientReaction {
    app_status: client::EventReaction,
    command_error: Option<String>,
}

impl ClientReaction {
    #[track_caller]
    pub fn expect_ok(&self) {
        self.app_status.expect_cont();
        if let Some(error) = &self.command_error {
            panic!("Expected no error, found \"{}\"", error)
        }
    }

    #[track_caller]
    pub fn expect_app_continue(&self) {
        self.expect_app_status(client::EventReaction::Continue);
    }

    #[track_caller]
    pub fn expect_app_status(&self, status: client::EventReaction) {
        assert_eq!(self.app_status, status);
    }

    #[track_caller]
    pub fn expect_error_contains(&self, substr: &str) {
        if let Some(error) = &self.command_error {
            assert!(error.contains(substr));
        } else {
            panic!("Expected command to fail with \"{}\", but it succeeded", substr);
        }
    }

    #[track_caller]
    pub fn expect_error_contains_dbg(&self, v: &impl fmt::Debug) {
        self.expect_error_contains(&format!("{:?}", v));
    }
}


struct Client {
    id: server::ClientId,
    incoming_rx: mpsc::Receiver<BughouseServerEvent>,
    outgoing_rx: mpsc::Receiver<BughouseClientEvent>,
    state: client::ClientState,
}

impl Client {
    pub fn new(my_name: String, my_team: Team, server: &mut Server) -> Self {
        let (incoming_tx, incoming_rx) = mpsc::channel();
        let (outgoing_tx, outgoing_rx) = mpsc::channel();
        let id = server.add_client(incoming_tx);
        let mut state = client::ClientState::new(my_name, my_team, outgoing_tx);
        state.join();
        Client{ id, incoming_rx, outgoing_rx, state }
    }

    fn my_name(&self) -> &str {
        self.state.my_name()
    }
    fn game_local(&self) -> BughouseGame {
        match &self.state.contest_state() {
            client::ContestState::Game{ game_confirmed, local_turn, .. } =>
                client::game_local(self.my_name(), game_confirmed, local_turn),
            _ => panic!("No game in found"),
        }
    }
    fn force(&self) -> Force {
        self.game_local().find_player(self.my_name()).unwrap().1
    }
    fn board(&self) -> Board {
        self.game_local().player_board(self.my_name()).unwrap().clone()
    }

    fn process_outgoing_events(&mut self, server: &mut Server) -> bool {
        let mut something_changed = false;
        for event in self.outgoing_rx.try_iter() {
            something_changed = true;
            println!("{:?} >>> {:?}", self.id, event);
            server.send_network_event(self.id, event);
        }
        something_changed
    }
    fn process_incoming_events(&mut self) -> bool {
        let mut something_changed = false;
        for event in self.incoming_rx.try_iter() {
            something_changed = true;
            println!("{:?} <<< {:?}", self.id, event);
            self.state.apply_event(client::IncomingEvent::Network(event)).expect_cont();
        }
        something_changed
    }

    fn send_key(&mut self, code: KeyCode, modifiers: KeyModifiers) -> ClientReaction {
        let ev = term_event::Event::Key(term_event::KeyEvent::new(code, modifiers));
        let app_status = self.state.apply_event(client::IncomingEvent::Terminal(ev));
        ClientReaction{ app_status, command_error: self.state.command_error().clone() }
    }
    fn execute_command(&mut self, cmd: &str) -> ClientReaction {
        for ch in cmd.chars() {
            self.send_key(KeyCode::Char(ch), KeyModifiers::empty()).expect_app_continue();
        }
        self.send_key(KeyCode::Enter, KeyModifiers::empty())
    }
    #[allow(dead_code)] fn tick(&mut self) {
        self.state.apply_event(client::IncomingEvent::Tick).expect_cont();
    }
}


#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
struct TestClientId(usize);

struct World {
    server: Server,
    // Note. Not using `HashMap<server::ClientId, Client>`, because `ClientId`s are meant
    //   to be recyclable and we don't want to reuse IDs in tests.
    clients: Vec<Client>,
}

impl World {
    fn new() -> Self {
        World {
            server: Server::new(),
            clients: vec![],
        }
    }

    fn add_client(&mut self, name: &str, team: Team) -> TestClientId {
        let idx = TestClientId(self.clients.len());
        self.clients.push(Client::new(name.to_owned(), team, &mut self.server));
        idx
    }

    fn process_events_for(&mut self, client_id: TestClientId) {
        let client = &mut self.clients[client_id.0];
        client.process_outgoing_events(&mut self.server);
        client.process_incoming_events();
    }
    fn process_events_from_clients(&mut self) -> bool {
        let mut something_changed = false;
        for client in &mut self.clients {
            if client.process_outgoing_events(&mut self.server) {
                something_changed = true;
            }
        }
        something_changed
    }
    fn process_events_to_clients(&mut self) -> bool {
        let mut something_changed = false;
        for client in &mut self.clients {
            if client.process_incoming_events() {
                something_changed = true;
            }
        }
        something_changed
    }
    // TODO: Randomize order to simulate network better.
    // TODO: When to tick?
    fn process_all_events(&mut self) {
        let mut something_changed = true;
        while something_changed {
            something_changed = false;
            if self.process_events_from_clients() {
                something_changed = true;
            }
            if self.process_events_to_clients() {
                something_changed = true;
            }
        }
    }
}

impl ops::Index<TestClientId> for World {
    type Output = Client;
    fn index(&self, id: TestClientId) -> &Self::Output { &self.clients[id.0] }
}
impl ops::IndexMut<TestClientId> for World {
    fn index_mut(&mut self, id: TestClientId) -> &mut Self::Output { &mut self.clients[id.0] }
}


// TODO: Consider name that easier to parse and less looking like chess coord,
//   e.g. paw, pab, pbw, pbb
#[test]
fn play_online() {
    let mut world = World::new();
    world.server.state.TEST_override_board_assignment(vec! [
        ("p1".to_owned(), BughouseBoard::A),
        ("p2".to_owned(), BughouseBoard::B),
        ("p3".to_owned(), BughouseBoard::A),
        ("p4".to_owned(), BughouseBoard::B),
    ]);

    let cl1 = world.add_client("p1", Team::Red);
    let cl2 = world.add_client("p2", Team::Red);
    let cl3 = world.add_client("p3", Team::Blue);

    world.process_all_events();
    assert!(matches!(world[cl1].state.contest_state(), client::ContestState::Lobby{ .. }));

    let cl4 = world.add_client("p4", Team::Blue);

    world.process_all_events();
    assert!(matches!(world[cl1].state.contest_state(), client::ContestState::Game{ .. }));

    // Input from inactive player is ignored
    world[cl3].execute_command("hello").expect_ok();

    world[cl1].execute_command("e5").expect_error_contains_dbg(&TurnError::Unreachable);
    world[cl1].execute_command("e4").expect_ok();
    world.process_all_events();

    // Now the invalid command is processed
    world[cl3].send_key(KeyCode::Enter, KeyModifiers::empty()).expect_error_contains("hello");

    world[cl3].execute_command("d5").expect_ok();
    world.process_all_events();

    world[cl1].execute_command("xd5").expect_ok();
    world.process_all_events();
    assert_eq!(world[cl2].board().reserve(world[cl2].force())[PieceKind::Pawn], 1);

    world[cl4].execute_command("Nc3").expect_ok();
    world.process_all_events();

    world[cl2].execute_command("P@e4").expect_ok();
    world.process_all_events();

    world[cl4].execute_command("d4").expect_ok();
    world.process_all_events();

    world[cl2].execute_command("xd3").expect_ok();  // en passant
    world.process_all_events();
}


// Regression test for turn preview bug: turns from the other boards could've been
// reverted when a local turn was confirmed.
#[test]
fn remote_turn_persisted() {
    let mut world = World::new();
    world.server.state.TEST_override_board_assignment(vec! [
        ("p1".to_owned(), BughouseBoard::A),
        ("p2".to_owned(), BughouseBoard::B),
        ("p3".to_owned(), BughouseBoard::A),
        ("p4".to_owned(), BughouseBoard::B),
    ]);

    let cl1 = world.add_client("p1", Team::Red);
    let _cl2 = world.add_client("p2", Team::Red);
    let _cl3 = world.add_client("p3", Team::Blue);
    let cl4 = world.add_client("p4", Team::Blue);

    world.process_all_events();

    world[cl1].execute_command("e4").expect_ok();
    world[cl4].execute_command("d4").expect_ok();
    world.process_events_for(cl4);
    world.process_events_for(cl1);
    assert!(world[cl1].game_local().board(BughouseBoard::B).grid()[Coord::D4].is_some());
}
