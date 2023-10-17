// Improvement potential. Test time-related things with mock clock.
// In particular, add regression test for trying to make a turn after time ran out
//   according to the client clock, but the server hasn't confirmed game over yet.

// Improvement potential. Cover all events, including RequestExport and ReportError.

mod common;

use std::collections::{HashMap, HashSet};
use std::sync::{mpsc, Arc, Mutex};
use std::{iter, ops};

use bughouse_chess::altered_game::AlteredGame;
use bughouse_chess::board::{Board, TurnError, TurnInput, VictoryReason};
use bughouse_chess::coord::{Coord, SubjectiveRow};
use bughouse_chess::display::{get_display_board_index, DisplayBoard, Perspective};
use bughouse_chess::event::{BughouseClientEvent, BughouseServerEvent};
use bughouse_chess::force::Force;
use bughouse_chess::game::{
    BughouseBoard, BughouseEnvoy, BughouseGame, BughouseGameStatus, BughouseParticipant,
    BughousePlayer, PlayerInGame,
};
use bughouse_chess::piece::PieceKind;
use bughouse_chess::player::{Faction, Team};
use bughouse_chess::rules::{
    BughouseRules, ChessRules, DropAggression, MatchRules, Promotion, Rules,
};
use bughouse_chess::server::ServerInfo;
use bughouse_chess::server_helpers::TestServerHelpers;
use bughouse_chess::session_store::SessionStore;
use bughouse_chess::{client, pgn, server};
use common::*;
use itertools::Itertools;
use BughouseBoard::{A, B};


fn default_chess_rules() -> ChessRules {
    ChessRules {
        bughouse_rules: Some(BughouseRules {
            koedem: false,
            promotion: Promotion::Upgrade,
            min_pawn_drop_rank: SubjectiveRow::from_one_based(2),
            max_pawn_drop_rank: SubjectiveRow::from_one_based(6),
            drop_aggression: DropAggression::NoChessMate,
        }),
        ..ChessRules::chess_blitz()
    }
}

fn single_player(name: &str, envoy: BughouseEnvoy) -> PlayerInGame {
    PlayerInGame {
        name: name.to_owned(),
        id: BughousePlayer::SinglePlayer(envoy),
    }
}

fn double_player(name: &str, team: Team) -> PlayerInGame {
    PlayerInGame {
        name: name.to_owned(),
        id: BughousePlayer::DoublePlayer(team),
    }
}


struct Server {
    clients: Arc<Mutex<server::Clients>>,
    state: server::ServerState,
}

impl Server {
    fn new() -> Self {
        let clients = Arc::new(Mutex::new(server::Clients::new()));
        let clients_copy = Arc::clone(&clients);
        let session_store = Arc::new(Mutex::new(SessionStore::new()));
        let server_info = Arc::new(Mutex::new(ServerInfo::new()));
        let mut state = server::ServerState::new(
            clients_copy,
            session_store,
            server_info,
            Box::new(TestServerHelpers {}),
            None,
        );
        state.TEST_disable_countdown();
        Server { clients, state }
    }

    fn add_client(&mut self, events_tx: mpsc::Sender<BughouseServerEvent>) -> server::ClientId {
        self.clients.lock().unwrap().add_client(events_tx, None, "client".to_owned())
    }

    fn send_network_event(&mut self, id: server::ClientId, event: BughouseClientEvent) {
        self.state.apply_event(server::IncomingEvent::Network(id, event));
    }
    #[allow(dead_code)]
    fn tick(&mut self) { self.state.apply_event(server::IncomingEvent::Tick); }
}


struct Client {
    id: Option<server::ClientId>,
    incoming_rx: Option<mpsc::Receiver<BughouseServerEvent>>,
    state: client::ClientState,
}

impl Client {
    pub fn new() -> Self {
        let user_agent = "Test".to_owned();
        let time_zone = "?".to_owned();
        let state = client::ClientState::new(user_agent, time_zone);
        Client { id: None, incoming_rx: None, state }
    }

    fn connect(&mut self, server: &mut Server) {
        let (incoming_tx, incoming_rx) = mpsc::channel();
        self.id = Some(server.add_client(incoming_tx));
        self.incoming_rx = Some(incoming_rx);
    }

    fn join(&mut self, match_id: &str, my_name: &str) {
        self.state.set_guest_player_name(Some(my_name.to_owned()));
        self.state.join(match_id.to_owned())
    }

    fn alt_game(&self) -> &AlteredGame { &self.state.game_state().unwrap().alt_game }
    fn perspective(&self) -> Perspective { self.alt_game().perspective() }
    fn my_id(&self) -> BughouseParticipant { self.alt_game().my_id() }
    fn local_game(&self) -> BughouseGame { self.alt_game().local_game() }

    // Only if player:
    fn my_player_id(&self) -> BughousePlayer { self.my_id().as_player().unwrap() }

    // Only if single player:
    fn my_envoy(&self) -> BughouseEnvoy {
        let BughousePlayer::SinglePlayer(envoy) = self.my_player_id() else {
            panic!("Not a single player");
        };
        envoy
    }
    fn my_force(&self) -> Force { self.my_envoy().force }
    fn my_board_idx(&self) -> BughouseBoard { self.my_envoy().board_idx }
    fn my_display_board_idx(&self) -> DisplayBoard {
        get_display_board_index(self.my_board_idx(), self.perspective())
    }
    fn my_board(&self) -> Board { self.local_game().board(self.my_board_idx()).clone() }
    fn other_board(&self) -> Board { self.local_game().board(self.my_board_idx().other()).clone() }
    fn make_turn(&mut self, turn: impl AutoTurnInput) -> Result<(), TurnError> {
        self.state.make_turn(self.my_display_board_idx(), turn.to_turn_input())
    }
    fn cancel_preturn(&mut self) { self.state.cancel_preturn(self.my_display_board_idx()) }

    fn process_outgoing_events(&mut self, server: &mut Server) -> bool {
        let mut something_changed = false;
        while let Some(event) = self.state.next_outgoing_event() {
            something_changed = true;
            println!("{:?} >>> {:?}", self.id.unwrap(), event);
            server.send_network_event(self.id.unwrap(), event);
        }
        something_changed
    }
    fn process_incoming_events(&mut self) -> (bool, Result<(), client::EventError>) {
        let mut something_changed = false;
        for event in self.incoming_rx.as_mut().unwrap().try_iter() {
            something_changed = true;
            println!("{:?} <<< {:?}", self.id.unwrap(), event);
            let result = self.state.process_server_event(event);
            if let Err(err) = result {
                return (something_changed, Err(err));
            }
        }
        self.state.refresh();
        (something_changed, Ok(()))
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
    fn new() -> Self { World { server: Server::new(), clients: vec![] } }

    fn new_match_with_rules(
        &mut self, client_id: TestClientId, player_name: &str, chess_rules: ChessRules,
    ) -> String {
        let rules = Rules {
            match_rules: MatchRules::unrated(),
            chess_rules,
        };
        self[client_id].state.set_guest_player_name(Some(player_name.to_owned()));
        self[client_id].state.new_match(rules);
        self.process_all_events();
        self[client_id].state.match_id().unwrap().clone()
    }
    fn new_match(&mut self, client_id: TestClientId, player_name: &str) -> String {
        self.new_match_with_rules(client_id, player_name, default_chess_rules())
    }

    fn join_and_set_team(
        &mut self, client_id: TestClientId, match_id: &str, player_name: &str, team: Team,
    ) {
        self[client_id].join(match_id, player_name);
        self.process_events_for(client_id).unwrap();
        self[client_id].state.set_faction(Faction::Fixed(team));
    }

    fn new_client(&mut self) -> TestClientId {
        let idx = TestClientId(self.clients.len());
        let mut client = Client::new();
        client.connect(&mut self.server);
        self.clients.push(client);
        idx
    }
    fn new_clients<const NUM: usize>(&mut self) -> [TestClientId; NUM] {
        iter::repeat_with(|| self.new_client())
            .take(NUM)
            .collect_vec()
            .try_into()
            .unwrap()
    }
    fn reconnect_client(&mut self, client_id: TestClientId) {
        let client = &mut self.clients[client_id.0];
        self.server.clients.lock().unwrap().remove_client(client.id.unwrap());
        client.connect(&mut self.server);
    }

    fn default_clients(
        &mut self,
    ) -> (String, TestClientId, TestClientId, TestClientId, TestClientId) {
        self.default_clients_with_rules(default_chess_rules())
    }
    fn default_clients_with_rules(
        &mut self, chess_rules: ChessRules,
    ) -> (String, TestClientId, TestClientId, TestClientId, TestClientId) {
        let [cl1, cl2, cl3, cl4] = self.new_clients();

        let mtch = self.new_match_with_rules(cl1, "p1", chess_rules);
        self[cl1].state.set_faction(Faction::Fixed(Team::Red));
        self.process_all_events();

        self.server.state.TEST_override_board_assignment(mtch.clone(), vec![
            single_player("p1", envoy!(White A)),
            single_player("p2", envoy!(Black B)),
            single_player("p3", envoy!(Black A)),
            single_player("p4", envoy!(White B)),
        ]);

        self.join_and_set_team(cl2, &mtch, "p2", Team::Red);
        self.join_and_set_team(cl3, &mtch, "p3", Team::Blue);
        self.join_and_set_team(cl4, &mtch, "p4", Team::Blue);
        self.process_all_events();

        for cl in [cl1, cl2, cl3, cl4].iter() {
            self[*cl].state.set_ready(true);
        }
        self.process_all_events();
        (mtch, cl1, cl2, cl3, cl4)
    }

    fn process_outgoing_events_for(&mut self, client_id: TestClientId) -> bool {
        self.clients[client_id.0].process_outgoing_events(&mut self.server)
    }
    fn process_incoming_events_for(
        &mut self, client_id: TestClientId,
    ) -> (bool, Result<(), client::EventError>) {
        self.clients[client_id.0].process_incoming_events()
    }
    fn process_events_for(&mut self, client_id: TestClientId) -> Result<(), client::EventError> {
        self.process_outgoing_events_for(client_id);
        self.process_incoming_events_for(client_id).1
    }
    fn process_events_from_clients(&mut self, ban_list: &HashSet<TestClientId>) -> bool {
        let mut something_changed = false;
        for (id, client) in self.clients.iter_mut().enumerate() {
            if ban_list.contains(&TestClientId(id)) {
                continue;
            }
            if client.process_outgoing_events(&mut self.server) {
                something_changed = true;
            }
        }
        something_changed
    }
    fn process_events_to_clients(&mut self, ban_list: &HashSet<TestClientId>) -> bool {
        let mut something_changed = false;
        for (id, client) in self.clients.iter_mut().enumerate() {
            if ban_list.contains(&TestClientId(id)) {
                continue;
            }
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
    fn process_all_events_except_clients(
        &mut self, ban_list: impl IntoIterator<Item = TestClientId>,
    ) {
        let ban_list = ban_list.into_iter().collect();
        let mut something_changed = true;
        while something_changed {
            something_changed = false;
            if self.process_events_from_clients(&ban_list) {
                something_changed = true;
            }
            if self.process_events_to_clients(&ban_list) {
                something_changed = true;
            }
        }
    }
    fn process_all_events(&mut self) { self.process_all_events_except_clients(iter::empty()); }

    fn replay_white_checkmates_black(&mut self, white_id: TestClientId, black_id: TestClientId) {
        self[white_id].make_turn("Nf3").unwrap();
        self.process_all_events();
        self[black_id].make_turn("h6").unwrap();
        self.process_all_events();
        self[white_id].make_turn("Ng5").unwrap();
        self.process_all_events();
        self[black_id].make_turn("h5").unwrap();
        self.process_all_events();
        self[white_id].make_turn("e4").unwrap();
        self.process_all_events();
        self[black_id].make_turn("h4").unwrap();
        self.process_all_events();
        self[white_id].make_turn("Qf3").unwrap();
        self.process_all_events();
        self[black_id].make_turn("h3").unwrap();
        self.process_all_events();
        self[white_id].make_turn("Qxf7").unwrap();
        self.process_all_events();
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

    let mtch = world.new_match(cl1, "p1");
    world[cl1].state.set_faction(Faction::Fixed(Team::Red));
    world.process_all_events();

    world.server.state.TEST_override_board_assignment(mtch.clone(), vec![
        single_player("p1", envoy!(White A)),
        single_player("p2", envoy!(Black B)),
        single_player("p3", envoy!(Black A)),
        single_player("p4", envoy!(White B)),
    ]);

    world.join_and_set_team(cl2, &mtch, "p2", Team::Red);
    world.join_and_set_team(cl3, &mtch, "p3", Team::Blue);
    world.join_and_set_team(cl4, &mtch, "p4", Team::Blue);
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

    world[cl2].make_turn("xd3").unwrap(); // en passant
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

    world[cl1].make_turn("e4").unwrap();
    world.process_all_events();
    world[cl3].make_turn("d5").unwrap();
    world.process_all_events();

    // Invalid pre-move ignored.
    world[cl3].make_turn("d4").unwrap();
    world.process_all_events();
    world[cl1].make_turn("d4").unwrap();
    world.process_all_events();
    assert!(world[cl1].my_board().grid()[Coord::D5].is(piece!(Black Pawn)));
    assert!(world[cl1].my_board().grid()[Coord::D4].is(piece!(White Pawn)));
}

// Regression test: `parse_drag_drop_turn` shouldn't panic if the piece was captured.
#[test]
fn preturn_failed_piece_captured() {
    let mut world = World::new();
    let (_, cl1, _cl2, cl3, _cl4) = world.default_clients();

    world[cl1].make_turn(drag_move!(E2 -> E4)).unwrap();
    world.process_all_events();
    world[cl3].make_turn(drag_move!(D7 -> D5)).unwrap();
    world.process_all_events();

    // Invalid pre-move ignored.
    world[cl3].make_turn(drag_move!(D5 -> D4)).unwrap();
    world.process_all_events();
    world[cl1].make_turn(drag_move!(E4 -> D5)).unwrap();
    world.process_all_events();
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
    world[cl3].cancel_preturn();
    world.process_all_events();
    world[cl1].make_turn("e4").unwrap();
    world.process_all_events();
    assert!(world[cl1].my_board().grid()[Coord::C6].is_none());

    world[cl3].make_turn("Nf6").unwrap();
    world.process_all_events();

    // Cancel pre-turn and schedule other
    world[cl3].make_turn("a7a6").unwrap();
    world.process_all_events();
    world[cl3].cancel_preturn();
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
    world[cl3].cancel_preturn();
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
// Regression test: reconnecting to a finished game where the client had a preturn shouldn't panic.
#[test]
fn preturn_auto_cancellation_on_checkmate() {
    let mut world = World::new();
    let (mtch, cl1, cl2, cl3, _cl4) = world.default_clients();

    world[cl2].make_turn("e5").unwrap();
    world.process_all_events();
    assert!(world[cl2].my_board().grid()[Coord::E5].is(piece!(Black Pawn)));

    world.replay_white_checkmates_black(cl1, cl3);
    assert!(world[cl2].my_board().grid()[Coord::E5].is_none());
    world[cl2].state.leave();
    world.process_all_events();

    let cl2_new = world.new_client();
    world[cl2_new].join(&mtch, "p2");
    world.process_all_events();
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
fn cold_reconnect_lobby() {
    let mut world = World::new();
    let [cl1, cl2, cl3] = world.new_clients();

    let mtch = world.new_match(cl1, "p1");
    world[cl1].state.set_faction(Faction::Fixed(Team::Red));
    world.join_and_set_team(cl2, &mtch, "p2", Team::Red);
    world.join_and_set_team(cl3, &mtch, "p3", Team::Blue);
    world.process_all_events();

    world.process_all_events();
    assert_eq!(world[cl1].state.mtch().unwrap().participants.len(), 3);

    world[cl2].state.leave();
    world[cl3].state.leave();
    world.process_all_events();
    assert_eq!(world[cl1].state.mtch().unwrap().participants.len(), 1);

    let cl4 = world.new_client();
    world.join_and_set_team(cl4, &mtch, "p4", Team::Blue);
    world.process_all_events();
    world[cl4].state.set_ready(true);
    world.process_all_events();
    // Game should not start yet because some players have been removed.
    assert!(world[cl1].state.game_state().is_none());
    assert_eq!(world[cl1].state.mtch().unwrap().participants.len(), 2);

    // Cannot reconnect as an active player.
    let cl1_new = world.new_client();
    world[cl1_new].join(&mtch, "p1");
    assert!(matches!(
        world.process_events_for(cl1_new),
        Err(client::EventError::IgnorableError(_))
    ));
    world.process_all_events();

    // Can reconnect with the same name - that's fine.
    let cl2_new = world.new_client();
    world.join_and_set_team(cl2_new, &mtch, "p2", Team::Red);
    // Can use free spot to connect with a different name - that's fine too.
    let cl5 = world.new_client();
    world.join_and_set_team(cl5, &mtch, "p5", Team::Blue);
    world.process_all_events();
    assert!(world[cl1].state.game_state().is_none());

    world[cl1].state.set_ready(true);
    world[cl2_new].state.set_ready(true);
    world[cl5].state.set_ready(true);
    world.process_all_events();
    assert!(world[cl1].state.game_state().is_some());
}

#[test]
fn cold_reconnect_game_active() {
    let mut world = World::new();
    let (mtch, cl1, _cl2, cl3, _cl4) = world.default_clients();
    assert!(world[cl1].state.game_state().is_some());

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
    assert!(world[cl1].state.game_state().is_some());

    // Can connect mid-game as an observer.
    let cl5 = world.new_client();
    world[cl5].join(&mtch, "p5");
    world.process_all_events();
    assert_eq!(world[cl5].state.mtch().unwrap().my_faction, Faction::Observer);

    // Cannot reconnect as an active player.
    let cl2_new = world.new_client();
    world[cl2_new].join(&mtch, "p2");
    assert!(matches!(
        world.process_events_for(cl2_new),
        Err(client::EventError::IgnorableError(_))
    ));
    world.process_all_events();

    // Reconnection successful.
    let cl3_new = world.new_client();
    world[cl3_new].join(&mtch, "p3");
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
fn cold_reconnect_game_over_checkmate() {
    let mut world = World::new();
    let (mtch, cl1, _cl2, cl3, cl4) = world.default_clients();

    world[cl4].make_turn("e4").unwrap();
    world.process_all_events();
    world[cl4].state.leave();
    world.process_all_events();

    world.replay_white_checkmates_black(cl1, cl3);
    let cl4_new = world.new_client();
    world[cl4_new].join(&mtch, "p4");
    world.process_all_events();
    assert!(world[cl4_new].my_board().grid()[Coord::E4].is(piece!(White Pawn)));
    assert_eq!(
        world[cl4_new].alt_game().status(),
        BughouseGameStatus::Victory(Team::Red, VictoryReason::Checkmate)
    );
}

#[test]
fn cold_reconnect_game_over_resignation() {
    let mut world = World::new();
    let (mtch, cl1, _cl2, _cl3, cl4) = world.default_clients();

    world[cl4].make_turn("e4").unwrap();
    world.process_all_events();
    world[cl4].state.leave();
    world.process_all_events();

    world[cl1].state.resign();
    world.process_all_events();
    let cl4_new = world.new_client();
    world[cl4_new].join(&mtch, "p4");
    world.process_all_events();
    assert!(world[cl4_new].my_board().grid()[Coord::E4].is(piece!(White Pawn)));
    assert_eq!(
        world[cl4_new].alt_game().status(),
        BughouseGameStatus::Victory(Team::Blue, VictoryReason::Resignation)
    );
}

#[test]
fn hot_reconnect_lobby() {
    let mut world = World::new();
    let [cl1, cl2, cl3] = world.new_clients();

    let mtch = world.new_match(cl1, "p1");
    world[cl1].state.set_faction(Faction::Fixed(Team::Red));
    world.join_and_set_team(cl2, &mtch, "p2", Team::Red);
    world.join_and_set_team(cl3, &mtch, "p3", Team::Blue);
    world.process_all_events();
    assert_eq!(world[cl1].state.mtch().unwrap().participants.len(), 3);

    world.reconnect_client(cl2);
    world[cl2].state.set_faction(Faction::Fixed(Team::Blue));
    world[cl2].state.set_ready(true);

    {
        let p = world[cl1]
            .state
            .mtch()
            .unwrap()
            .participants
            .iter()
            .find(|p| p.name == "p2")
            .unwrap();
        assert_eq!(p.faction, Faction::Fixed(Team::Red));
        assert!(!p.is_ready);
    }

    world[cl2].state.hot_reconnect();
    world.process_all_events();

    // TODO:
    // {
    //     let p = world[cl1]
    //         .state
    //         .mtch()
    //         .unwrap()
    //         .participants
    //         .iter()
    //         .find(|p| p.name == "p2")
    //         .unwrap();
    //     assert_eq!(p.faction, Faction::Fixed(Team::Blue));
    //     assert!(p.is_ready);
    // }
}

// Test a situation when WebSocket connection was lost due to a network issue, but the client is
// still running. The client should be able to continue making and cancelling turns in the meantime.
#[test]
fn hot_reconnect_game_active() {
    let mut world = World::new();
    let (_, cl1, cl2, cl3, cl4) = world.default_clients();

    world[cl1].make_turn("e4").unwrap();
    world.process_all_events();
    world[cl3].make_turn("e5").unwrap();
    world.process_all_events();

    world[cl3].make_turn("Nc6").unwrap();
    world.process_all_events();

    // TODO: Test a scenario when server doesn't realize the connect was lost until the last moment.
    world.reconnect_client(cl3);
    world[cl3].cancel_preturn();
    world[cl3].make_turn("Nf6").unwrap();

    world[cl2].make_turn("d5").unwrap();
    world.process_all_events_except_clients([cl3]);
    world[cl4].make_turn("d4").unwrap();
    world.process_all_events_except_clients([cl3]);

    world[cl3].state.hot_reconnect();
    world.process_all_events();

    world[cl1].make_turn("f4").unwrap();
    world.process_all_events();

    // Make sure turns were re-applied properly, including the turn cl1 made after disconnection:
    for cl in [cl1, cl2, cl3, cl4] {
        let game = world[cl].local_game();
        let grid_a = game.board(A).grid();
        let grid_b = game.board(B).grid();
        assert!(grid_a[Coord::E4].is(piece!(White Pawn)));
        assert!(grid_a[Coord::E5].is(piece!(Black Pawn)));
        assert!(grid_a[Coord::F4].is(piece!(White Pawn)));
        assert!(grid_a[Coord::C6].is_none());
        assert!(grid_a[Coord::F6].is(piece!(Black Knight)));
        assert!(grid_b[Coord::D4].is(piece!(White Pawn)));
        assert!(grid_b[Coord::D4].is(piece!(White Pawn)));
    }
}

// Similar to `preturn_cancellation_late`, but simulates a sutiation when we tried to cancel a
// preturn and make a new one while WebSocket connection was interrupted. Note that the new local
// preturn should be cancelled after restoring the connection.
#[test]
fn hot_reconnect_preturn_cancellation_late() {
    let mut world = World::new();
    let (_, cl1, cl2, cl3, cl4) = world.default_clients();

    world[cl1].make_turn("e4").unwrap();
    world.process_all_events();
    world[cl3].make_turn("e5").unwrap();
    world.process_all_events();

    world[cl3].make_turn("Nc6").unwrap();
    world.process_all_events();

    world.reconnect_client(cl3);
    world[cl3].cancel_preturn();
    world[cl3].make_turn("Nf6").unwrap();

    world[cl1].make_turn("f4").unwrap();
    world.process_all_events_except_clients([cl3]);

    {
        let grid = world[cl3].local_game().board(A).grid().clone();
        assert!(grid[Coord::C6].is_none());
        assert!(grid[Coord::F6].is(piece!(Black Knight)));
    }

    world[cl3].state.hot_reconnect();
    world.process_all_events();

    for cl in [cl1, cl2, cl3, cl4] {
        let grid = world[cl].local_game().board(A).grid().clone();
        assert!(grid[Coord::C6].is(piece!(Black Knight)));
        assert!(grid[Coord::F6].is_none());
    }
}

#[test]
fn hot_reconnect_observer() {
    let mut world = World::new();
    let (mtch, cl1, _cl2, _cl3, _cl4) = world.default_clients();

    let cl5 = world.new_client();
    world[cl5].join(&mtch, "p5");
    world.process_all_events();

    world.reconnect_client(cl5);

    world[cl1].make_turn("e4").unwrap();
    world.process_all_events_except_clients([cl5]);

    world[cl5].state.hot_reconnect();
    world.process_all_events();
    assert!(world[cl5].local_game().board(A).grid()[Coord::E4].is(piece!(White Pawn)));
}

#[test]
fn hot_reconnect_game_over() {
    let mut world = World::new();
    let (_, cl1, _cl2, _cl3, cl4) = world.default_clients();

    world.reconnect_client(cl4);
    world[cl1].state.resign();
    world.process_all_events_except_clients([cl4]);
    world[cl4].state.hot_reconnect();
    world.process_all_events();
    assert_eq!(
        world[cl4].alt_game().status(),
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
    world.process_all_events();
}

// Even if multiple turns have been made on the other board, the promo steal should still execute
// properly thanks to the piece ID tracking.
#[test]
fn high_latency_stealing() {
    let mut rules = default_chess_rules();
    rules.bughouse_rules.as_mut().unwrap().promotion = Promotion::Steal;
    let mut world = World::new();
    let (_, cl1, cl2, cl3, cl4) = world.default_clients_with_rules(rules);

    world[cl4].make_turn("Nc3").unwrap();
    world.process_all_events();

    world[cl1].make_turn("a4").unwrap();
    world.process_all_events();
    world[cl3].make_turn("h5").unwrap();
    world.process_all_events();
    world[cl1].make_turn("a5").unwrap();
    world.process_all_events();
    world[cl3].make_turn("h4").unwrap();
    world.process_all_events();
    world[cl1].make_turn("a6").unwrap();
    world.process_all_events();
    world[cl3].make_turn("h3").unwrap();
    world.process_all_events();
    world[cl1].make_turn("xb7").unwrap();
    world.process_all_events();
    world[cl3].make_turn("xg2").unwrap();
    world.process_all_events();
    // Improvement potential: track piece ID for algebraic notation (see `PromotionTarget::Steal`
    // comment) and replace `drag_move!(...)` with "xc8=Nc3" for a more end-to-end experience
    // (without manual piece ID lookup).
    let steal_target = world[cl1].local_game().board(B).grid()[Coord::C3].unwrap();
    world[cl1].make_turn(drag_move!(B7 -> C8 = steal_target)).unwrap();

    world[cl2].make_turn("e5").unwrap();
    world.process_events_for(cl2).unwrap();
    world[cl4].make_turn("Nb5").unwrap();
    world.process_events_for(cl4).unwrap();
    world[cl2].make_turn("d5").unwrap();
    world.process_events_for(cl2).unwrap();
    world[cl4].make_turn("Nxc7").unwrap();
    world.process_events_for(cl4).unwrap();
    world.process_events_for(cl2).unwrap();

    assert!(world[cl2].local_game().board(B).grid()[Coord::C7].is(piece!(White Knight)));
    world.process_all_events();
    assert!(world[cl2].local_game().board(B).grid()[Coord::C7].is_none());
}

#[test]
fn three_players() {
    use DisplayBoard::*;

    let mut world = World::new();
    let [cl1, cl2, cl3] = world.new_clients();

    let mtch = world.new_match(cl1, "p1");

    world.server.state.TEST_override_board_assignment(mtch.clone(), vec![
        single_player("p1", envoy!(White A)),
        single_player("p2", envoy!(Black B)),
        double_player("p3", Team::Blue),
    ]);

    world[cl2].join(&mtch, "p2");
    world[cl3].join(&mtch, "p3");
    world.process_all_events();

    for cl in [cl1, cl2, cl3].iter() {
        world[*cl].state.set_ready(true);
    }
    world.process_all_events();

    // For a double-player the board where they play White is always primary, thus
    // for p3: A is Secondary, B is Primary.
    world[cl1].make_turn("e4").unwrap();
    world[cl3]
        .state
        .make_turn(Secondary, TurnInput::Algebraic("e5".to_owned()))
        .unwrap();
    world[cl3]
        .state
        .make_turn(Primary, TurnInput::Algebraic("Nc3".to_owned()))
        .unwrap();
    world.process_all_events();
    assert!(world[cl2].local_game().board(A).grid()[Coord::E4].is(piece!(White Pawn)));
    assert!(world[cl2].local_game().board(A).grid()[Coord::E5].is(piece!(Black Pawn)));
    assert!(world[cl2].local_game().board(B).grid()[Coord::C3].is(piece!(White Knight)));
}

#[test]
fn five_players() {
    let mut world = World::new();
    let [cl1, cl2, cl3, cl4, cl5] = world.new_clients();

    let mtch = world.new_match(cl1, "p1");

    world.server.state.TEST_override_board_assignment(mtch.clone(), vec![
        single_player("p1", envoy!(White A)),
        single_player("p2", envoy!(Black B)),
        single_player("p3", envoy!(Black A)),
        single_player("p4", envoy!(White B)),
    ]);

    world[cl2].join(&mtch, "p2");
    world[cl3].join(&mtch, "p3");
    world[cl4].join(&mtch, "p4");
    world[cl5].join(&mtch, "p5");
    world.process_all_events();

    for cl in [cl1, cl2, cl3, cl4, cl5].iter() {
        world[*cl].state.set_ready(true);
    }
    world.process_all_events();

    // The player who does not participate should still be able to see the game.
    world[cl1].make_turn("e4").unwrap();
    world.process_all_events();
    assert!(world[cl5].local_game().board(A).grid()[Coord::E4].is(piece!(White Pawn)));
}

#[test]
fn two_matches() {
    let mut world = World::new();
    let [cl1, cl2, cl3, cl4, cl5, cl6, cl7, cl8] = world.new_clients();

    let match1 = world.new_match(cl1, "p1");
    world[cl1].state.set_faction(Faction::Fixed(Team::Red));
    world.process_all_events();
    let match2 = world.new_match(cl5, "p5");
    world[cl5].state.set_faction(Faction::Fixed(Team::Red));
    world.process_all_events();

    world.server.state.TEST_override_board_assignment(match1.clone(), vec![
        single_player("p1", envoy!(White A)),
        single_player("p2", envoy!(Black B)),
        single_player("p3", envoy!(Black A)),
        single_player("p4", envoy!(White B)),
    ]);
    world.server.state.TEST_override_board_assignment(match2.clone(), vec![
        single_player("p5", envoy!(White A)),
        single_player("p6", envoy!(Black B)),
        single_player("p7", envoy!(Black A)),
        single_player("p8", envoy!(White B)),
    ]);

    world.join_and_set_team(cl2, &match1, "p2", Team::Red);
    world.join_and_set_team(cl3, &match1, "p3", Team::Blue);
    world.join_and_set_team(cl4, &match1, "p4", Team::Blue);
    world.join_and_set_team(cl6, &match2, "p6", Team::Red);
    world.join_and_set_team(cl7, &match2, "p7", Team::Blue);
    world.join_and_set_team(cl8, &match2, "p8", Team::Blue);
    world.process_all_events();

    for cl in [cl1, cl2, cl3, cl4, cl5, cl6, cl7, cl8].iter() {
        world[*cl].state.set_ready(true);
    }
    world.process_all_events();

    world[cl1].make_turn("e4").unwrap();
    world[cl5].make_turn("Nc3").unwrap();
    world.process_all_events();
    assert!(world[cl2].local_game().board(A).grid()[Coord::E4].is(piece!(White Pawn)));
    assert!(world[cl2].local_game().board(A).grid()[Coord::C3].is_none());
    assert!(world[cl6].local_game().board(A).grid()[Coord::E4].is_none());
    assert!(world[cl6].local_game().board(A).grid()[Coord::C3].is(piece!(White Knight)));
}

#[test]
fn seating_assignment_is_fair() {
    let mut world = World::new();
    let [cl1, cl2, cl3, cl4, cl5, cl6] = world.new_clients();

    let mtch = world.new_match(cl1, "p1");
    world[cl2].join(&mtch, "p2");
    world[cl3].join(&mtch, "p3");
    world[cl4].join(&mtch, "p4");
    world[cl5].join(&mtch, "p5");
    world[cl6].join(&mtch, "p6");
    world.process_all_events();

    world[cl6].state.set_faction(Faction::Observer);
    world.process_all_events();

    let mut games_played = HashMap::new();
    for _ in 0..100 {
        for cl in [cl1, cl2, cl3, cl4, cl5].iter() {
            world[*cl].state.set_ready(true);
        }
        world.process_all_events();
        for cl in [cl1, cl2, cl3, cl4, cl5].iter() {
            if world[*cl].my_id() != BughouseParticipant::Observer {
                world[*cl].state.resign();
                break;
            }
        }
        world.process_all_events();
        for p in world[cl1].local_game().players() {
            *games_played.entry(p.name.clone()).or_default() += 1;
        }
    }
    assert_eq!(
        games_played,
        HashMap::from([
            ("p1".to_owned(), 80),
            ("p2".to_owned(), 80),
            ("p3".to_owned(), 80),
            ("p4".to_owned(), 80),
            ("p5".to_owned(), 80),
        ])
    );
}

// Verify conformity to PGN standard.
#[test]
fn pgn_standard() {
    let mut world = World::new();
    let (_, cl1, _cl2, cl3, _cl4) = world.default_clients();

    world[cl1].make_turn("e4").unwrap();
    world.process_all_events();
    world[cl3].make_turn("e5").unwrap();
    world.process_all_events();
    world[cl1].make_turn("Nf3").unwrap();
    world.process_all_events();
    world[cl3].make_turn("Nc6").unwrap();
    world.process_all_events();
    world[cl1].make_turn("g3").unwrap();
    world.process_all_events();
    world[cl3].make_turn("d5").unwrap();
    world.process_all_events();
    world[cl1].make_turn("Bg2").unwrap();
    world.process_all_events();
    world[cl3].make_turn("Qe7").unwrap();
    world.process_all_events();
    world[cl1].make_turn("Nxe5").unwrap();
    world.process_all_events();
    world[cl3].make_turn("xe4").unwrap();
    world.process_all_events();
    world[cl1].make_turn("0-0").unwrap();
    world.process_all_events();

    world[cl1].state.request_export(pgn::BughouseExportFormat {});
    world.process_all_events();
    while let Some(event) = world[cl1].state.next_notable_event() {
        if let client::NotableEvent::GameExportReady(content) = event {
            println!("Got PGN:\n{content}");
            // Test: Uses short algebraic and includes capture notations.
            assert!(content.contains(" Nx"));
            // Test: Does not contain non-ASCII characters (like "×").
            assert!(content.chars().all(|ch| ch.is_ascii()));
            // Test: Castling is PGN-style (not FIDE-style).
            assert!(content.contains("O-O"));
            assert!(!content.contains("0-0"));
            return;
        }
    }
    panic!("Did not get the PGN");
}
