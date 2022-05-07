// Improvement potential. Test time-related things with mock clock.

use std::ops;
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use bughouse_chess::*;
use bughouse_chess::client::TurnCommandError::IllegalTurn;


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
        self.clients.lock().unwrap().add_client(events_tx, "client".to_owned())
    }

    fn send_network_event(&mut self, id: server::ClientId, event: BughouseClientEvent) {
        self.state.apply_event(server::IncomingEvent::Network(id, event));
    }
    #[allow(dead_code)] fn tick(&mut self) {
        self.state.apply_event(server::IncomingEvent::Tick);
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
        let state = client::ClientState::new(my_name, my_team, outgoing_tx);
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
    fn my_force(&self) -> Force {
        self.game_local().find_player(self.my_name()).unwrap().1
    }
    fn my_board(&self) -> Board {
        self.game_local().player_board(self.my_name()).unwrap().clone()
    }
    fn other_board(&self) -> Board {
        let game_local = self.game_local();
        let (my_board_idx, _) = game_local.find_player(self.my_name()).unwrap();
        game_local.board(my_board_idx.other()).clone()
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
    fn process_incoming_events(&mut self) -> (bool, Result<(), client::EventError>) {
        let mut something_changed = false;
        for event in self.incoming_rx.try_iter() {
            something_changed = true;
            println!("{:?} <<< {:?}", self.id, event);
            let result = self.state.process_server_event(event);
            if let Err(err) = result {
                return (something_changed, Err(err));
            }
        }
        (something_changed, Ok(()))
    }
    fn make_turn(&mut self, turn_algebraic: &str) -> Result<(), client::TurnCommandError> {
        self.state.make_turn(turn_algebraic.to_owned())
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
        let mut client = Client::new(name.to_owned(), team, &mut self.server);
        client.state.join();
        self.clients.push(client);
        idx
    }

    fn process_events_for(&mut self, client_id: TestClientId) -> Result<(), client::EventError> {
        let client = &mut self.clients[client_id.0];
        client.process_outgoing_events(&mut self.server);
        client.process_incoming_events().1
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
            let (change, reaction) = client.process_incoming_events();
            reaction.unwrap();
            if change {
                something_changed = true;
            }
        }
        something_changed
    }
    // Improvement potential: Randomize order to simulate network better.
    // Improvement potential: Consider if this need to auto-tick (maybe randomly).
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


// Improvement potential. Consider names that are easier to parse and don't look like
//   chess coord, e.g. "paw", "pab", "pbw", "pbb".
#[test]
fn play_online_misc() {
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

    // Input from inactive player is not parsed.
    assert_eq!(world[cl3].make_turn("attack!").unwrap_err(), IllegalTurn(TurnError::WrongTurnOrder));

    assert_eq!(world[cl1].make_turn("e5").unwrap_err(), IllegalTurn(TurnError::Unreachable));
    world[cl1].make_turn("e4").unwrap();
    world.process_all_events();

    world[cl3].make_turn("d5").unwrap();
    world.process_all_events();

    world[cl1].make_turn("xd5").unwrap();
    world.process_all_events();
    assert_eq!(world[cl2].my_board().reserve(world[cl2].my_force())[PieceKind::Pawn], 1);

    world[cl4].make_turn("Nc3").unwrap();
    world.process_all_events();

    world[cl2].make_turn("P@e4").unwrap();
    world.process_all_events();

    world[cl4].make_turn("d4").unwrap();
    world.process_all_events();

    world[cl2].make_turn("xd3").unwrap();  // en passant
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

    world[cl1].make_turn("e4").unwrap();
    world[cl4].make_turn("d4").unwrap();
    world.process_events_for(cl4).unwrap();
    world.process_events_for(cl1).unwrap();
    assert!(world[cl1].game_local().board(BughouseBoard::B).grid()[Coord::D4].is_some());
}

#[test]
fn leave_and_reconnect_lobby() {
    let mut world = World::new();

    let cl1 = world.add_client("p1", Team::Red);
    let cl2 = world.add_client("p2", Team::Red);
    let cl3 = world.add_client("p3", Team::Blue);
    world.process_all_events();
    match world[cl1].state.contest_state() {
        client::ContestState::Lobby{ players, .. } => assert_eq!(players.len(), 3),
        _ => panic!("Expected client to be in Lobby state"),
    }

    world[cl2].state.leave();
    world[cl3].state.leave();
    world.process_all_events();
    match world[cl1].state.contest_state() {
        client::ContestState::Lobby{ players, .. } => assert_eq!(players.len(), 1),
        _ => panic!("Expected client to be in Lobby state"),
    }

    let _cl4 = world.add_client("p4", Team::Blue);
    world.process_all_events();
    // Game should not start yet because some players have been removed.
    match world[cl1].state.contest_state() {
        client::ContestState::Lobby{ players, .. } => assert_eq!(players.len(), 2),
        _ => panic!("Expected client to be in Lobby state"),
    }

    // Cannot reconnect as an active player.
    let cl1_new = world.add_client("p1", Team::Blue);
    assert!(matches!(world.process_events_for(cl1_new), Err(client::EventError::ServerReturnedError(_))));
    world.process_all_events();

    // Can reconnect with the same name - that's fine.
    let _cl2_new = world.add_client("p2", Team::Red);
    // Can use free spot to connect with a different name - that's fine too.
    let _cl5 = world.add_client("p5", Team::Blue);
    world.process_all_events();
    assert!(matches!(world[cl1].state.contest_state(), client::ContestState::Game{ .. }));
}

#[test]
fn leave_and_reconnect_game() {
    use PieceKind::*;

    let mut world = World::new();
    world.server.state.TEST_override_board_assignment(vec! [
        ("p1".to_owned(), BughouseBoard::A),
        ("p2".to_owned(), BughouseBoard::B),
        ("p3".to_owned(), BughouseBoard::A),
        ("p4".to_owned(), BughouseBoard::B),
    ]);

    let cl1 = world.add_client("p1", Team::Red);
    let _cl2 = world.add_client("p2", Team::Red);
    let cl3 = world.add_client("p3", Team::Blue);
    let _cl4 = world.add_client("p4", Team::Blue);
    world.process_all_events();
    assert!(matches!(world[cl1].state.contest_state(), client::ContestState::Game{ .. }));

    world[cl1].make_turn("e4").unwrap();
    world.process_all_events();
    world[cl3].make_turn("d5").unwrap();
    world.process_all_events();
    world[cl1].make_turn("xd5").unwrap();
    world.process_all_events();
    world[cl3].make_turn("Nf6").unwrap();
    world.process_all_events();

    world[cl3].state.leave();
    world.process_all_events();
    // Show must go on - the game has started.
    assert!(matches!(world[cl1].state.contest_state(), client::ContestState::Game{ .. }));

    // Cannot connect as a different player even though somebody has left.
    let cl5 = world.add_client("p5", Team::Blue);
    assert!(matches!(world.process_events_for(cl5), Err(client::EventError::ServerReturnedError(_))));
    world.process_all_events();

    // Cannot reconnect as an active player.
    let cl2_new = world.add_client("p2", Team::Blue);
    assert!(matches!(world.process_events_for(cl2_new), Err(client::EventError::ServerReturnedError(_))));
    world.process_all_events();

    // Cannot reconnect as a different team.
    let cl3_new = world.add_client("p3", Team::Red);
    assert!(matches!(world.process_events_for(cl3_new), Err(client::EventError::ServerReturnedError(_))));
    world.process_all_events();

    // Reconnection successful.
    let cl3_new = world.add_client("p3", Team::Blue);
    world.process_events_for(cl3_new).unwrap();
    world.process_all_events();

    // Make sure turns were re-applied properly:
    let grid = world[cl3_new].my_board().grid().clone();
    let my_force = world[cl3_new].my_force();
    assert!(grid[Coord::E2].is_none());
    assert!(grid[Coord::E4].is_none());
    assert!(grid[Coord::D7].is_none());
    assert!(matches!(grid[Coord::D5], Some(PieceOnBoard{ kind: Pawn, .. })));
    assert!(matches!(grid[Coord::F6], Some(PieceOnBoard{ kind: Knight, .. })));
    assert_eq!(world[cl3_new].other_board().reserve(my_force)[Pawn], 1);
}

// Regression test: server should not panic when a client tries to make a turn after the
// game was over on another board.
#[test]
fn turn_after_game_ended_on_another_board() {
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
    assert!(matches!(world[cl1].state.contest_state(), client::ContestState::Game{ .. }));

    world[cl1].state.resign();
    world.process_events_for(cl1).unwrap();

    world[cl4].make_turn("e4").unwrap();
    assert!(matches!(world.process_events_for(cl4), Err(client::EventError::ServerReturnedError(_))));
    world.process_all_events();
}
