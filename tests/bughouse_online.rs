// Improvement potential. Test time-related things with mock clock.
// In particular, add regression test for trying to make a turn after time ran out
//   according to the client clock, but the server hasn't confirmed game over yet.

mod common;

use std::ops;
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use itertools::Itertools;

use bughouse_chess::*;
use bughouse_chess::client::TurnCommandError::IllegalTurn;
use common::*;


macro_rules! seating {
    ($force:ident $board_idx:ident) => {
        bughouse_chess::BughousePlayerId {
            board_idx: bughouse_chess::BughouseBoard::$board_idx,
            force: bughouse_chess::Force::$force,
        }
    };
}

fn default_chess_rules() -> ChessRules {
    ChessRules {
        starting_position: StartingPosition::Classic,
        time_control: TimeControl{ starting_time: Duration::from_secs(300) },
    }
}

fn default_bughouse_rules() -> BughouseRules {
    BughouseRules {
        teaming: Teaming::FixedTeams,
        min_pawn_drop_row: SubjectiveRow::from_one_based(2),
        max_pawn_drop_row: SubjectiveRow::from_one_based(6),
        drop_aggression: DropAggression::NoChessMate,
    }
}


struct Server {
    clients: Arc<Mutex<server::Clients>>,
    state: server::ServerState,
}

impl Server {
    fn new(chess_rules: ChessRules, bughouse_rules: BughouseRules) -> Self {
        let clients = Arc::new(Mutex::new(server::Clients::new()));
        let clients_copy = Arc::clone(&clients);
        let state = server::ServerState::new(clients_copy, chess_rules, bughouse_rules, None);
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
    pub fn new(my_name: String, my_team: Option<Team>, server: &mut Server) -> Self {
        let (incoming_tx, incoming_rx) = mpsc::channel();
        let (outgoing_tx, outgoing_rx) = mpsc::channel();
        let id = server.add_client(incoming_tx);
        let user_agent = "Test".to_owned();
        let time_zone = "?".to_owned();
        let state = client::ClientState::new(my_name, my_team, user_agent, time_zone, outgoing_tx);
        Client{ id, incoming_rx, outgoing_rx, state }
    }

    fn alt_game(&self) -> &AlteredGame { &self.state.game_state().unwrap().alt_game }
    fn my_id(&self) -> BughouseParticipantId { self.alt_game().my_id() }
    fn my_player_id(&self) -> BughousePlayerId {
        let BughouseParticipantId::Player(id) = self.my_id() else {
            panic!("Not a player");
        };
        id
    }
    fn local_game(&self) -> BughouseGame { self.alt_game().local_game() }
    fn my_force(&self) -> Force { self.my_player_id().force }
    fn my_board(&self) -> Board {
        self.local_game().board(self.my_player_id().board_idx).clone()
    }
    fn other_board(&self) -> Board {
        self.local_game().board(self.my_player_id().board_idx.other()).clone()
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
        self.state.refresh();
        (something_changed, Ok(()))
    }
    fn make_turn(&mut self, turn: impl AutoTurnInput) -> Result<(), client::TurnCommandError> {
        self.state.make_turn(turn.to_turn_input())
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
        Self::new_with_rules(default_chess_rules(), default_bughouse_rules())
    }
    fn new_with_rules(chess_rules: ChessRules, bughouse_rules: BughouseRules) -> Self {
        World {
            server: Server::new(chess_rules, bughouse_rules),
            clients: vec![],
        }
    }

    fn add_client(&mut self, name: &str, team: Option<Team>) -> TestClientId {
        let idx = TestClientId(self.clients.len());
        let mut client = Client::new(name.to_owned(), team, &mut self.server);
        client.state.join();
        self.clients.push(client);
        idx
    }
    fn default_clients(&mut self) -> (TestClientId, TestClientId, TestClientId, TestClientId) {
        self.server.state.TEST_override_board_assignment(vec! [
            ("p1".to_owned(), seating!(White A)),
            ("p2".to_owned(), seating!(Black B)),
            ("p3".to_owned(), seating!(Black A)),
            ("p4".to_owned(), seating!(White B)),
        ]);
        let mut clients = [
            self.add_client("p1", Some(Team::Red)),
            self.add_client("p2", Some(Team::Red)),
            self.add_client("p3", Some(Team::Blue)),
            self.add_client("p4", Some(Team::Blue)),
        ];
        self.process_all_events();
        for cl in clients.iter_mut() {
            self[*cl].state.set_ready(true);
        }
        self.process_all_events();
        clients.into_iter().collect_tuple().unwrap()
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

    fn replay_white_checkmates_black(&mut self, white_id: TestClientId, black_id: TestClientId) {
        self[white_id].make_turn("Nf3").unwrap();   self.process_all_events();
        self[black_id].make_turn("h6").unwrap();    self.process_all_events();
        self[white_id].make_turn("Ng5").unwrap();   self.process_all_events();
        self[black_id].make_turn("h5").unwrap();    self.process_all_events();
        self[white_id].make_turn("e4").unwrap();    self.process_all_events();
        self[black_id].make_turn("h4").unwrap();    self.process_all_events();
        self[white_id].make_turn("Qf3").unwrap();   self.process_all_events();
        self[black_id].make_turn("h3").unwrap();    self.process_all_events();
        self[white_id].make_turn("Qxf7").unwrap();  self.process_all_events();
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
        ("p1".to_owned(), seating!(White A)),
        ("p2".to_owned(), seating!(Black B)),
        ("p3".to_owned(), seating!(Black A)),
        ("p4".to_owned(), seating!(White B)),
    ]);

    let cl1 = world.add_client("p1", Some(Team::Red));
    let cl2 = world.add_client("p2", Some(Team::Red));
    let cl3 = world.add_client("p3", Some(Team::Blue));
    let cl4 = world.add_client("p4", Some(Team::Blue));
    world.process_all_events();

    world[cl1].state.set_ready(true);
    world[cl2].state.set_ready(true);
    world[cl3].state.set_ready(true);
    world.process_all_events();
    assert!(world[cl1].state.game_state().is_none());

    world[cl4].state.set_ready(true);
    world.process_all_events();
    assert!(world[cl1].state.game_state().is_some());

    assert_eq!(world[cl1].make_turn("e5").unwrap_err(), IllegalTurn(TurnError::ImpossibleTrajectory));
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
    let (cl1, _cl2, _cl3, cl4) = world.default_clients();

    world[cl1].make_turn("e4").unwrap();
    world[cl4].make_turn("d4").unwrap();
    world.process_events_for(cl4).unwrap();
    world.process_events_for(cl1).unwrap();
    assert!(world[cl1].other_board().grid()[Coord::D4].is(piece!(White Pawn)));
}

#[test]
fn preturn_successful() {
    let mut world = World::new();
    let (cl1, _cl2, cl3, _cl4) = world.default_clients();

    // Valid pre-move executed after opponent's turn.
    world[cl3].make_turn("d5").unwrap();
    world.process_all_events();
    assert!(world[cl1].my_board().grid()[Coord::D5].is_none());
    world[cl1].make_turn("e4").unwrap();
    world.process_all_events();
    assert!(world[cl1].my_board().grid()[Coord::D5].is(piece!(Black Pawn)));
}

#[test]
fn preturn_failed_square_occupied() {
    let mut world = World::new();
    let (cl1, _cl2, cl3, _cl4) = world.default_clients();

    world[cl1].make_turn("e4").unwrap();  world.process_all_events();
    world[cl3].make_turn("d5").unwrap();  world.process_all_events();

    // Invalid pre-move ignored.
    world[cl3].make_turn("d4").unwrap();  world.process_all_events();
    world[cl1].make_turn("d4").unwrap();  world.process_all_events();
    assert!(world[cl1].my_board().grid()[Coord::D5].is(piece!(Black Pawn)));
    assert!(world[cl1].my_board().grid()[Coord::D4].is(piece!(White Pawn)));
}

// Regression test: `parse_drag_drop_turn` shouldn't panic if the piece was captured.
#[test]
fn preturn_failed_piece_captured() {
    let mut world = World::new();
    let (cl1, _cl2, cl3, _cl4) = world.default_clients();

    world[cl1].make_turn(drag_move!(E2 -> E4)).unwrap();  world.process_all_events();
    world[cl3].make_turn(drag_move!(D7 -> D5)).unwrap();  world.process_all_events();

    // Invalid pre-move ignored.
    world[cl3].make_turn(drag_move!(D5 -> D4)).unwrap();  world.process_all_events();
    world[cl1].make_turn(drag_move!(E4 -> D5)).unwrap();  world.process_all_events();
    assert!(world[cl1].my_board().grid()[Coord::D5].is(piece!(White Pawn)));
    assert!(world[cl1].my_board().grid()[Coord::D4].is_none());
}

#[test]
fn preturn_cancellation() {
    let mut world = World::new();
    let (cl1, _cl2, cl3, _cl4) = world.default_clients();

    // Cancel pre-turn
    world[cl3].make_turn("Nc6").unwrap();
    world.process_all_events();
    world[cl3].state.cancel_preturn();
    world.process_all_events();
    world[cl1].make_turn("e4").unwrap();
    world.process_all_events();
    assert!(world[cl1].my_board().grid()[Coord::C6].is_none());

    world[cl3].make_turn("Nf6").unwrap();
    world.process_all_events();

    // Cancel pre-turn and schedule other
    world[cl3].make_turn("a7a6").unwrap();
    world.process_all_events();
    world[cl3].state.cancel_preturn();
    world.process_all_events();
    world[cl3].make_turn("h7h6").unwrap();
    world.process_all_events();
    world[cl1].make_turn("d4").unwrap();
    world.process_all_events();
    assert!(world[cl1].my_board().grid()[Coord::A6].is_none());
    assert!(world[cl1].my_board().grid()[Coord::H6].is(piece!(Black Pawn)));
}

// Regression test: having preturn when game ends shouldn't panic.
#[test]
fn preturn_auto_cancellation_on_resign() {
    let mut world = World::new();
    let (cl1, cl2, _cl3, _cl4) = world.default_clients();

    world[cl2].make_turn("e5").unwrap();
    world.process_all_events();
    assert!(world[cl2].my_board().grid()[Coord::E5].is(piece!(Black Pawn)));

    world[cl1].state.resign();
    world.process_all_events();
    assert!(world[cl2].my_board().grid()[Coord::E5].is_none());
}

// Regression test: having preturn when game ends shouldn't panic.
#[test]
fn preturn_auto_cancellation_on_checkmate() {
    let mut world = World::new();
    let (cl1, cl2, cl3, _cl4) = world.default_clients();

    world[cl2].make_turn("e5").unwrap();
    world.process_all_events();
    assert!(world[cl2].my_board().grid()[Coord::E5].is(piece!(Black Pawn)));

    world.replay_white_checkmates_black(cl1, cl3);
    assert!(world[cl2].my_board().grid()[Coord::E5].is_none());
}

#[test]
fn reconnect_lobby() {
    let mut world = World::new();

    let cl1 = world.add_client("p1", Some(Team::Red));
    let cl2 = world.add_client("p2", Some(Team::Red));
    let cl3 = world.add_client("p3", Some(Team::Blue));
    world.process_all_events();
    world[cl1].state.set_ready(true);
    world[cl2].state.set_ready(true);
    world[cl3].state.set_ready(true);
    world.process_all_events();
    assert_eq!(world[cl1].state.contest().unwrap().players.len(), 3);

    world[cl2].state.leave();
    world[cl3].state.leave();
    world.process_all_events();
    assert_eq!(world[cl1].state.contest().unwrap().players.len(), 1);

    let cl4 = world.add_client("p4", Some(Team::Blue));
    world.process_all_events();
    world[cl4].state.set_ready(true);
    world.process_all_events();
    // Game should not start yet because some players have been removed.
    assert!(world[cl1].state.game_state().is_none());
    assert_eq!(world[cl1].state.contest().unwrap().players.len(), 2);

    // Cannot reconnect as an active player.
    let cl1_new = world.add_client("p1", Some(Team::Blue));
    assert!(matches!(world.process_events_for(cl1_new), Err(client::EventError::ServerReturnedError(_))));
    world.process_all_events();

    // Can reconnect with the same name - that's fine.
    let cl2_new = world.add_client("p2", Some(Team::Red));
    // Can use free spot to connect with a different name - that's fine too.
    let cl5 = world.add_client("p5", Some(Team::Blue));
    world.process_all_events();
    assert!(world[cl1].state.game_state().is_none());

    world[cl2_new].state.set_ready(true);
    world[cl5].state.set_ready(true);
    world.process_all_events();
    assert!(world[cl1].state.game_state().is_some());
}

#[test]
fn reconnect_game_active() {
    let mut world = World::new();
    let (cl1, _cl2, cl3, _cl4) = world.default_clients();
    assert!(world[cl1].state.game_state().is_some());

    world[cl1].make_turn("e4").unwrap();   world.process_all_events();
    world[cl3].make_turn("d5").unwrap();   world.process_all_events();
    world[cl1].make_turn("xd5").unwrap();  world.process_all_events();
    world[cl3].make_turn("Nf6").unwrap();  world.process_all_events();

    world[cl3].state.leave();
    world.process_all_events();
    // Show must go on - the game has started.
    assert!(world[cl1].state.game_state().is_some());

    // Cannot connect as a different player even though somebody has left.
    let cl5 = world.add_client("p5", Some(Team::Blue));
    assert!(matches!(world.process_events_for(cl5), Err(client::EventError::ServerReturnedError(_))));
    world.process_all_events();

    // Cannot reconnect as an active player.
    let cl2_new = world.add_client("p2", Some(Team::Blue));
    assert!(matches!(world.process_events_for(cl2_new), Err(client::EventError::ServerReturnedError(_))));
    world.process_all_events();

    // Cannot reconnect as a different team.
    let cl3_new = world.add_client("p3", Some(Team::Red));
    assert!(matches!(world.process_events_for(cl3_new), Err(client::EventError::ServerReturnedError(_))));
    world.process_all_events();

    // Reconnection successful.
    let cl3_new = world.add_client("p3", Some(Team::Blue));
    world.process_events_for(cl3_new).unwrap();
    world.process_all_events();

    // Make sure turns were re-applied properly:
    let grid = world[cl3_new].my_board().grid().clone();
    let my_force = world[cl3_new].my_force();
    assert!(grid[Coord::E2].is_none());
    assert!(grid[Coord::E4].is_none());
    assert!(grid[Coord::D7].is_none());
    assert!(grid[Coord::D5].is(piece!(White Pawn)));
    assert!(grid[Coord::F6].is(piece!(Black Knight)));
    assert_eq!(world[cl3_new].other_board().reserve(my_force)[PieceKind::Pawn], 1);
}

#[test]
fn reconnect_game_over_checkmate() {
    let mut world = World::new();
    let (cl1, _cl2, cl3, cl4) = world.default_clients();

    world[cl4].make_turn("e4").unwrap();
    world.process_all_events();
    world[cl4].state.leave();
    world.process_all_events();

    world.replay_white_checkmates_black(cl1, cl3);
    let cl4_new = world.add_client("p4", Some(Team::Blue));
    world.process_all_events();
    assert!(world[cl4_new].my_board().grid()[Coord::E4].is(piece!(White Pawn)));
    assert_eq!(
        world[cl4_new].alt_game().status(),
        BughouseGameStatus::Victory(Team::Red, VictoryReason::Checkmate)
    );
}

#[test]
fn reconnect_game_over_resignation() {
    let mut world = World::new();
    let (cl1, _cl2, _cl3, cl4) = world.default_clients();

    world[cl4].make_turn("e4").unwrap();
    world.process_all_events();
    world[cl4].state.leave();
    world.process_all_events();

    world[cl1].state.resign();
    world.process_all_events();
    let cl4_new = world.add_client("p4", Some(Team::Blue));
    world.process_all_events();
    assert!(world[cl4_new].my_board().grid()[Coord::E4].is(piece!(White Pawn)));
    assert_eq!(
        world[cl4_new].alt_game().status(),
        BughouseGameStatus::Victory(Team::Blue, VictoryReason::Resignation)
    );
}

// Regression test: server should not panic when a client tries to make a turn after the
// game was over on another board.
#[test]
fn turn_after_game_ended_on_another_board() {
    let mut world = World::new();
    let (cl1, _cl2, _cl3, cl4) = world.default_clients();
    assert!(world[cl1].state.game_state().is_some());

    world[cl1].state.resign();
    world.process_events_for(cl1).unwrap();

    world[cl4].make_turn("e4").unwrap();
    assert!(matches!(world.process_events_for(cl4), Err(client::EventError::ServerReturnedError(_))));
    world.process_all_events();
}

#[test]
fn game_reset() {
    let mut world = World::new();
    let (cl1, cl2, cl3, cl4) = world.default_clients();

    world.replay_white_checkmates_black(cl1, cl3);
    world.process_all_events();
    assert_eq!(world[cl2].state.contest().unwrap().scores.per_team.get(&Team::Red), Some(&2));

    world[cl4].state.reset();
    world.process_all_events();
    assert_eq!(world[cl2].state.contest().unwrap().scores.per_team.get(&Team::Red), None);
}

#[test]
fn five_players() {
    let mut world = World::new_with_rules(
        default_chess_rules(),
        BughouseRules {
            teaming: Teaming::IndividualMode,
            .. default_bughouse_rules()
        }
    );

    world.server.state.TEST_override_board_assignment(vec! [
        ("p1".to_owned(), seating!(White A)),
        ("p2".to_owned(), seating!(Black B)),
        ("p3".to_owned(), seating!(Black A)),
        ("p4".to_owned(), seating!(White B)),
    ]);
    let mut clients = [
        world.add_client("p1", None),
        world.add_client("p2", None),
        world.add_client("p3", None),
        world.add_client("p4", None),
        world.add_client("p5", None),
    ];
    world.process_all_events();
    for cl in clients.iter_mut() {
        world[*cl].state.set_ready(true);
    }
    world.process_all_events();
    let (cl1, _cl2, _cl3, _cl4, cl5) = clients.into_iter().collect_tuple().unwrap();

    // The player who does not participate should still be able to see the game.
    world[cl1].make_turn("e4").unwrap();
    world.process_all_events();
    assert!(world[cl5].local_game().board(BughouseBoard::A).grid()[Coord::E4].is(piece!(White Pawn)));
}
