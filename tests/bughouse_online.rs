// Improvement potential. Test time-related things with mock clock.
// In particular, add regression test for trying to make a turn after time ran out
//   according to the client clock, but the server hasn't confirmed game over yet.

// Improvement potential. Cover all events, including RequestExport and ReportError.

mod common;

use std::iter;
use std::ops;
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use itertools::Itertools;

use bughouse_chess::*;
use common::*;


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

fn player_in_game(name: &str, id: BughousePlayerId) -> PlayerInGame {
    PlayerInGame{ name: name.to_owned(), id }
}


struct Server {
    clients: Arc<Mutex<server::Clients>>,
    state: server::ServerState,
}

impl Server {
    fn new() -> Self {
        let clients = Arc::new(Mutex::new(server::Clients::new()));
        let clients_copy = Arc::clone(&clients);
        let state = server::ServerState::new(clients_copy, None);
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
    pub fn new(server: &mut Server) -> Self {
        let (incoming_tx, incoming_rx) = mpsc::channel();
        let (outgoing_tx, outgoing_rx) = mpsc::channel();
        let id = server.add_client(incoming_tx);
        let user_agent = "Test".to_owned();
        let time_zone = "?".to_owned();
        let state = client::ClientState::new(user_agent, time_zone, outgoing_tx);
        Client{ id, incoming_rx, outgoing_rx, state }
    }

    fn join(&mut self, contest_id: &str, my_name: &str) {
        self.state.join(contest_id.to_owned(), my_name.to_owned())
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
    fn make_turn(&mut self, turn: impl AutoTurnInput) -> Result<(), TurnError> {
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
        World {
            server: Server::new(),
            clients: vec![],
        }
    }

    fn new_contest_with_rules(
        &mut self, client_id: TestClientId, player_name: &str,
        chess_rules: ChessRules, bughouse_rules: BughouseRules
    ) -> String {
        self[client_id].state.new_contest(
            ContestCreationOptions {
                chess_rules,
                bughouse_rules,
                player_name:
                player_name.to_owned(),
                rated: true });
        self.process_all_events();
        self[client_id].state.contest_id().unwrap().clone()
    }
    fn new_contest(&mut self, client_id: TestClientId, player_name: &str) -> String {
        self.new_contest_with_rules(
            client_id, player_name, default_chess_rules(), default_bughouse_rules()
        )
    }

    fn join_and_set_team(
        &mut self, client_id: TestClientId, contest_id: &str, player_name: &str, team: Team
    ) {
        self[client_id].join(contest_id, player_name);
        self.process_events_for(client_id).unwrap();
        self[client_id].state.set_team(team);
    }

    fn new_client(&mut self) -> TestClientId {
        let idx = TestClientId(self.clients.len());
        let client = Client::new(&mut self.server);
        self.clients.push(client);
        idx
    }
    fn new_clients<const NUM: usize>(&mut self) -> [TestClientId; NUM] {
        iter::repeat_with(|| self.new_client()).take(NUM).collect_vec().try_into().unwrap()
    }

    fn default_clients(&mut self) -> (String, TestClientId, TestClientId, TestClientId, TestClientId) {
        let [cl1, cl2, cl3, cl4] = self.new_clients();

        let contest = self.new_contest(cl1, "p1");
        self[cl1].state.set_team(Team::Red);
        self.process_all_events();

        self.server.state.TEST_override_board_assignment(contest.clone(), vec! [
            player_in_game("p1", seating!(White A)),
            player_in_game("p2", seating!(Black B)),
            player_in_game("p3", seating!(Black A)),
            player_in_game("p4", seating!(White B)),
        ]);

        self.join_and_set_team(cl2, &contest, "p2", Team::Red);
        self.join_and_set_team(cl3, &contest, "p3", Team::Blue);
        self.join_and_set_team(cl4, &contest, "p4", Team::Blue);
        self.process_all_events();

        for cl in [cl1, cl2, cl3, cl4].iter() {
            self[*cl].state.set_ready(true);
        }
        self.process_all_events();
        (contest, cl1, cl2, cl3, cl4)
    }


    fn process_outgoing_events_for(&mut self, client_id: TestClientId) -> bool {
        self.clients[client_id.0].process_outgoing_events(&mut self.server)
    }
    fn process_incoming_events_for(&mut self, client_id: TestClientId) -> (bool, Result<(), client::EventError>) {
        self.clients[client_id.0].process_incoming_events()
    }
    fn process_events_for(&mut self, client_id: TestClientId) -> Result<(), client::EventError> {
        self.process_outgoing_events_for(client_id);
        self.process_incoming_events_for(client_id).1
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
    let [cl1, cl2, cl3, cl4] = world.new_clients();

    let contest = world.new_contest(cl1, "p1");
    world[cl1].state.set_team(Team::Red);
    world.process_all_events();

    world.server.state.TEST_override_board_assignment(contest.clone(), vec! [
        player_in_game("p1", seating!(White A)),
        player_in_game("p2", seating!(Black B)),
        player_in_game("p3", seating!(Black A)),
        player_in_game("p4", seating!(White B)),
    ]);

    world.join_and_set_team(cl2, &contest, "p2", Team::Red);
    world.join_and_set_team(cl3, &contest, "p3", Team::Blue);
    world.join_and_set_team(cl4, &contest, "p4", Team::Blue);
    world.process_all_events();

    world[cl1].state.set_ready(true);
    world[cl2].state.set_ready(true);
    world[cl3].state.set_ready(true);
    world.process_all_events();
    assert!(world[cl1].state.game_state().is_none());

    world[cl4].state.set_ready(true);
    world.process_all_events();
    assert!(world[cl1].state.game_state().is_some());

    assert_eq!(world[cl1].make_turn("e5").unwrap_err(), TurnError::ImpossibleTrajectory);
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
    let (_, cl1, _cl2, _cl3, cl4) = world.default_clients();

    world[cl1].make_turn("e4").unwrap();
    world[cl4].make_turn("d4").unwrap();
    world.process_events_for(cl4).unwrap();
    world.process_events_for(cl1).unwrap();
    assert!(world[cl1].other_board().grid()[Coord::D4].is(piece!(White Pawn)));
}

#[test]
fn preturn_successful() {
    let mut world = World::new();
    let (_, cl1, _cl2, cl3, _cl4) = world.default_clients();

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
    let (_, cl1, _cl2, cl3, _cl4) = world.default_clients();

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
    let (_, cl1, _cl2, cl3, _cl4) = world.default_clients();

    world[cl1].make_turn(drag_move!(E2 -> E4)).unwrap();  world.process_all_events();
    world[cl3].make_turn(drag_move!(D7 -> D5)).unwrap();  world.process_all_events();

    // Invalid pre-move ignored.
    world[cl3].make_turn(drag_move!(D5 -> D4)).unwrap();  world.process_all_events();
    world[cl1].make_turn(drag_move!(E4 -> D5)).unwrap();  world.process_all_events();
    assert!(world[cl1].my_board().grid()[Coord::D5].is(piece!(White Pawn)));
    assert!(world[cl1].my_board().grid()[Coord::D4].is_none());
}

#[test]
fn preturn_cancellation_successful() {
    let mut world = World::new();
    let (_, cl1, _cl2, cl3, _cl4) = world.default_clients();

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

#[test]
fn preturn_cancellation_late() {
    let mut world = World::new();
    let (_, cl1, _cl2, cl3, _cl4) = world.default_clients();

    world[cl3].make_turn("e5").unwrap();
    world.process_events_for(cl3).unwrap();
    world[cl3].state.cancel_preturn();
    world[cl3].make_turn("d5").unwrap();
    world[cl1].make_turn("Nc3").unwrap();
    world.process_events_for(cl1).unwrap();
    assert!(world[cl3].my_board().grid()[Coord::E5].is_none());
    assert!(world[cl3].my_board().grid()[Coord::D5].is(piece!(Black Pawn)));

    world.process_all_events();
    assert!(world[cl3].my_board().grid()[Coord::E5].is(piece!(Black Pawn)));
    assert!(world[cl3].my_board().grid()[Coord::D5].is_none());
}

// Regression test: having preturn when game ends shouldn't panic.
#[test]
fn preturn_auto_cancellation_on_resign() {
    let mut world = World::new();
    let (_, cl1, cl2, _cl3, _cl4) = world.default_clients();

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
    let (_, cl1, cl2, cl3, _cl4) = world.default_clients();

    world[cl2].make_turn("e5").unwrap();
    world.process_all_events();
    assert!(world[cl2].my_board().grid()[Coord::E5].is(piece!(Black Pawn)));

    world.replay_white_checkmates_black(cl1, cl3);
    assert!(world[cl2].my_board().grid()[Coord::E5].is_none());
}

#[test]
fn two_local_turns_both_successful() {
    let mut world = World::new();
    let (_, cl1, _cl2, cl3, _cl4) = world.default_clients();

    world[cl1].make_turn("e4").unwrap();
    world[cl1].make_turn("d4").unwrap();
    world[cl3].make_turn("e5").unwrap();
    assert!(world[cl1].my_board().grid()[Coord::E4].is(piece!(White Pawn)));
    assert!(world[cl3].my_board().grid()[Coord::E4].is_none());
    assert!(world[cl1].my_board().grid()[Coord::D4].is(piece!(White Pawn)));
    assert!(world[cl3].my_board().grid()[Coord::D4].is_none());
    assert!(world[cl1].my_board().grid()[Coord::E5].is_none());
    assert!(world[cl3].my_board().grid()[Coord::E5].is(piece!(Black Pawn)));

    world.process_all_events();
    assert!(world[cl1].my_board().grid()[Coord::E4].is(piece!(White Pawn)));
    assert!(world[cl3].my_board().grid()[Coord::E4].is(piece!(White Pawn)));
    assert!(world[cl1].my_board().grid()[Coord::D4].is(piece!(White Pawn)));
    assert!(world[cl3].my_board().grid()[Coord::D4].is(piece!(White Pawn)));
    assert!(world[cl1].my_board().grid()[Coord::E5].is(piece!(Black Pawn)));
    assert!(world[cl3].my_board().grid()[Coord::E5].is(piece!(Black Pawn)));
}

#[test]
fn two_local_turns_keep_preturn() {
    let mut world = World::new();
    let (_, cl1, _cl2, cl3, _cl4) = world.default_clients();

    world[cl1].make_turn("e4").unwrap();
    world.process_outgoing_events_for(cl1);
    world[cl1].make_turn("d4").unwrap();
    world[cl3].make_turn("e5").unwrap();
    world.process_events_for(cl3).unwrap();
    world.process_incoming_events_for(cl1).1.unwrap();
    assert!(world[cl1].my_board().grid()[Coord::E4].is(piece!(White Pawn)));
    assert!(world[cl3].my_board().grid()[Coord::E4].is(piece!(White Pawn)));
    assert!(world[cl1].my_board().grid()[Coord::D4].is(piece!(White Pawn)));
    assert!(world[cl3].my_board().grid()[Coord::D4].is_none());
    assert!(world[cl1].my_board().grid()[Coord::E5].is(piece!(Black Pawn)));
    assert!(world[cl3].my_board().grid()[Coord::E5].is(piece!(Black Pawn)));
}

#[test]
fn reconnect_lobby() {
    let mut world = World::new();
    let [cl1, cl2, cl3] = world.new_clients();

    let contest = world.new_contest(cl1, "p1");
    world[cl1].state.set_team(Team::Red);
    world.join_and_set_team(cl2, &contest, "p2", Team::Red);
    world.join_and_set_team(cl3, &contest, "p3", Team::Blue);
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

    let cl4 = world.new_client();
    world.join_and_set_team(cl4, &contest, "p4", Team::Blue);
    world.process_all_events();
    world[cl4].state.set_ready(true);
    world.process_all_events();
    // Game should not start yet because some players have been removed.
    assert!(world[cl1].state.game_state().is_none());
    assert_eq!(world[cl1].state.contest().unwrap().players.len(), 2);

    // Cannot reconnect as an active player.
    let cl1_new = world.new_client();
    world[cl1_new].join(&contest, "p1");
    assert!(matches!(world.process_events_for(cl1_new), Err(client::EventError::ServerReturnedError(_))));
    world.process_all_events();

    // Can reconnect with the same name - that's fine.
    let cl2_new = world.new_client();
    world.join_and_set_team(cl2_new, &contest, "p2", Team::Red);
    // Can use free spot to connect with a different name - that's fine too.
    let cl5 = world.new_client();
    world.join_and_set_team(cl5, &contest, "p5", Team::Blue);
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
    let (contest, cl1, _cl2, cl3, _cl4) = world.default_clients();
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
    let cl5 = world.new_client();
    world[cl5].join(&contest, "p5");
    assert!(matches!(world.process_events_for(cl5), Err(client::EventError::ServerReturnedError(_))));
    world.process_all_events();

    // Cannot reconnect as an active player.
    let cl2_new = world.new_client();
    world[cl2_new].join(&contest, "p2");
    assert!(matches!(world.process_events_for(cl2_new), Err(client::EventError::ServerReturnedError(_))));
    world.process_all_events();

    // Reconnection successful.
    let cl3_new = world.new_client();
    world[cl3_new].join(&contest, "p3");
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
    let (contest, cl1, _cl2, cl3, cl4) = world.default_clients();

    world[cl4].make_turn("e4").unwrap();
    world.process_all_events();
    world[cl4].state.leave();
    world.process_all_events();

    world.replay_white_checkmates_black(cl1, cl3);
    let cl4_new = world.new_client();
    world[cl4_new].join(&contest, "p4");
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
    let (contest, cl1, _cl2, _cl3, cl4) = world.default_clients();

    world[cl4].make_turn("e4").unwrap();
    world.process_all_events();
    world[cl4].state.leave();
    world.process_all_events();

    world[cl1].state.resign();
    world.process_all_events();
    let cl4_new = world.new_client();
    world[cl4_new].join(&contest, "p4");
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
    let (_, cl1, _cl2, _cl3, cl4) = world.default_clients();
    assert!(world[cl1].state.game_state().is_some());

    world[cl1].state.resign();
    world.process_events_for(cl1).unwrap();

    world[cl4].make_turn("e4").unwrap();
    assert!(matches!(world.process_events_for(cl4), Err(client::EventError::ServerReturnedError(_))));
    world.process_all_events();
}

#[test]
fn five_players() {
    let mut world = World::new();
    let [cl1, cl2, cl3, cl4, cl5] = world.new_clients();

    let contest = world.new_contest_with_rules(
        cl1, "p1",
        default_chess_rules(),
        BughouseRules {
            teaming: Teaming::IndividualMode,
            .. default_bughouse_rules()
        }
    );

    world.server.state.TEST_override_board_assignment(contest.clone(), vec! [
        player_in_game("p1", seating!(White A)),
        player_in_game("p2", seating!(Black B)),
        player_in_game("p3", seating!(Black A)),
        player_in_game("p4", seating!(White B)),
    ]);

    world[cl2].join(&contest, "p2");
    world[cl3].join(&contest, "p3");
    world[cl4].join(&contest, "p4");
    world[cl5].join(&contest, "p5");
    world.process_all_events();

    for cl in [cl1, cl2, cl3, cl4, cl5].iter() {
        world[*cl].state.set_ready(true);
    }
    world.process_all_events();

    // The player who does not participate should still be able to see the game.
    world[cl1].make_turn("e4").unwrap();
    world.process_all_events();
    assert!(world[cl5].local_game().board(BughouseBoard::A).grid()[Coord::E4].is(piece!(White Pawn)));
}

#[test]
fn two_contests() {
    let mut world = World::new();
    let [cl1, cl2, cl3, cl4, cl5, cl6, cl7, cl8] = world.new_clients();

    let contest1 = world.new_contest(cl1, "p1");
    world[cl1].state.set_team(Team::Red);
    world.process_all_events();
    let contest2 = world.new_contest(cl5, "p5");
    world[cl5].state.set_team(Team::Red);
    world.process_all_events();

    world.server.state.TEST_override_board_assignment(contest1.clone(), vec! [
        player_in_game("p1", seating!(White A)),
        player_in_game("p2", seating!(Black B)),
        player_in_game("p3", seating!(Black A)),
        player_in_game("p4", seating!(White B)),
    ]);
    world.server.state.TEST_override_board_assignment(contest2.clone(), vec! [
        player_in_game("p5", seating!(White A)),
        player_in_game("p6", seating!(Black B)),
        player_in_game("p7", seating!(Black A)),
        player_in_game("p8", seating!(White B)),
    ]);

    world.join_and_set_team(cl2, &contest1, "p2", Team::Red);
    world.join_and_set_team(cl3, &contest1, "p3", Team::Blue);
    world.join_and_set_team(cl4, &contest1, "p4", Team::Blue);
    world.join_and_set_team(cl6, &contest2, "p6", Team::Red);
    world.join_and_set_team(cl7, &contest2, "p7", Team::Blue);
    world.join_and_set_team(cl8, &contest2, "p8", Team::Blue);
    world.process_all_events();

    for cl in [cl1, cl2, cl3, cl4, cl5, cl6, cl7, cl8].iter() {
        world[*cl].state.set_ready(true);
    }
    world.process_all_events();

    world[cl1].make_turn("e4").unwrap();
    world[cl5].make_turn("Nc3").unwrap();
    world.process_all_events();
    assert!(world[cl2].local_game().board(BughouseBoard::A).grid()[Coord::E4].is(piece!(White Pawn)));
    assert!(world[cl2].local_game().board(BughouseBoard::A).grid()[Coord::C3].is_none());
    assert!(world[cl6].local_game().board(BughouseBoard::A).grid()[Coord::E4].is_none());
    assert!(world[cl6].local_game().board(BughouseBoard::A).grid()[Coord::C3].is(piece!(White Knight)));
}
