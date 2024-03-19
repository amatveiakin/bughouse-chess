// Improvement potential. Test time-related things with mock clock.
// In particular, add regression test for trying to make a turn after time ran out
//   according to the client clock, but the server hasn't confirmed game over yet.

// Improvement potential. Cover all events, including RequestExport and ReportError.

mod common;

use std::collections::{HashMap, HashSet};
use std::ops;
use std::sync::{Arc, Mutex};

use bughouse_chess::altered_game::{AlteredGame, WaybackDestination};
use bughouse_chess::board::{Board, TurnError, TurnInput, VictoryReason};
use bughouse_chess::chat::ChatRecipient;
use bughouse_chess::clock::GameInstant;
use bughouse_chess::coord::{Coord, SubjectiveRow};
use bughouse_chess::display::{get_display_board_index, DisplayBoard, Perspective};
use bughouse_chess::event::{BughouseClientEvent, BughouseServerEvent};
use bughouse_chess::force::Force;
use bughouse_chess::game::{
    BughouseBoard, BughouseEnvoy, BughouseGame, BughouseGameStatus, BughouseParticipant,
    BughousePlayer, PlayerInGame, TurnIndex,
};
use bughouse_chess::half_integer::HalfU32;
use bughouse_chess::piece::PieceKind;
use bughouse_chess::player::{Faction, Team};
use bughouse_chess::rules::{
    BughouseRules, ChessRules, DropAggression, MatchRules, PawnDropRanks, Promotion, Rules,
};
use bughouse_chess::scores::Scores;
use bughouse_chess::server::{ServerInfo, ServerOptions};
use bughouse_chess::server_helpers::TestServerHelpers;
use bughouse_chess::session::{RegistrationMethod, Session, UserInfo};
use bughouse_chess::session_store::{SessionId, SessionStore};
use bughouse_chess::utc_time::UtcDateTime;
use bughouse_chess::{client, pgn, server};
use common::*;
use instant::Instant;
use itertools::Itertools;
use time::Duration;
use BughouseBoard::{A, B};
use Force::{Black, White};


fn default_chess_rules() -> ChessRules {
    ChessRules {
        bughouse_rules: Some(BughouseRules {
            koedem: false,
            promotion: Promotion::Upgrade,
            pawn_drop_ranks: PawnDropRanks {
                min: SubjectiveRow::from_one_based(2),
                max: SubjectiveRow::from_one_based(6),
            },
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
    creation_instant: Instant,
    time_elapsed: Duration,
    next_session_id: usize,
    session_store: Arc<Mutex<SessionStore>>,
    clients: Arc<Mutex<server::Clients>>,
    state: server::ServerState,
}

impl Server {
    fn new() -> Self {
        let options = ServerOptions {
            check_git_version: false,
            max_starting_time: None,
        };
        let clients = Arc::new(Mutex::new(server::Clients::new()));
        let session_store = Arc::new(Mutex::new(SessionStore::new()));
        let server_info = Arc::new(Mutex::new(ServerInfo::new()));
        let mut state = server::ServerState::new(
            options,
            Arc::clone(&clients),
            Arc::clone(&session_store),
            server_info,
            Box::new(TestServerHelpers {}),
            None,
        );
        state.TEST_disable_countdown();
        state.TEST_disable_connection_health_check();
        Server {
            creation_instant: Instant::now(),
            time_elapsed: Duration::ZERO,
            next_session_id: 1,
            session_store,
            clients,
            state,
        }
    }

    fn set_time(&mut self, time: Duration) { self.time_elapsed = time; }
    fn current_instant(&self) -> Instant { self.creation_instant + self.time_elapsed }

    fn signin_user(&mut self, user_name: &str) -> SessionId {
        let session_id = SessionId::new(self.next_session_id.to_string());
        self.next_session_id += 1;
        let user_info = Session::LoggedIn(UserInfo {
            user_name: user_name.to_owned(),
            email: None,
            registration_method: RegistrationMethod::Password,
        });
        self.session_store.lock().unwrap().set(session_id.clone(), user_info);
        session_id
    }

    fn add_client(
        &mut self, events_tx: async_std::channel::Sender<BughouseServerEvent>,
        session_id: Option<SessionId>,
    ) -> server::ClientId {
        self.clients
            .lock()
            .unwrap()
            .add_client(events_tx, session_id, "client".to_owned())
    }

    fn send_network_event(&mut self, id: server::ClientId, event: BughouseClientEvent) {
        self.state.apply_event(
            server::IncomingEvent::Network(id, event),
            self.current_instant(),
            UtcDateTime::now(),
        );
    }
    fn tick(&mut self) {
        println!(">>> Tick");
        self.state.apply_event(
            server::IncomingEvent::Tick,
            self.current_instant(),
            UtcDateTime::now(),
        );
    }
}


struct Client {
    id: Option<server::ClientId>,
    incoming_rx: Option<async_std::channel::Receiver<BughouseServerEvent>>,
    state: client::ClientState,
}

impl Client {
    pub fn new() -> Self {
        let user_agent = "Test".to_owned();
        let time_zone = "?".to_owned();
        let state = client::ClientState::new(user_agent, time_zone);
        Client { id: None, incoming_rx: None, state }
    }

    fn connect(&mut self, server: &mut Server, session_id: Option<SessionId>) {
        let (incoming_tx, incoming_rx) = async_std::channel::unbounded();
        self.id = Some(server.add_client(incoming_tx, session_id));
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

    fn chat_item_text(&self) -> Vec<String> {
        let my_name = self.state.my_name().unwrap();
        let chess_rules = &self.state.mtch().unwrap().rules.chess_rules;
        let game_index = self.state.game_state().map(|state| state.game_index);
        self.state
            .mtch()
            .unwrap()
            .chat
            .items(my_name, chess_rules, game_index)
            .into_iter()
            .map(|item| item.text)
            .collect_vec()
    }

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
        while let Ok(event) = self.incoming_rx.as_mut().unwrap().try_recv() {
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

enum ClientFilter {
    All,
    Only(HashSet<TestClientId>),
    Except(HashSet<TestClientId>),
}

impl ClientFilter {
    fn all() -> Self { ClientFilter::All }
    fn only(ids: impl IntoIterator<Item = TestClientId>) -> Self {
        ClientFilter::Only(ids.into_iter().collect())
    }
    fn except(ids: impl IntoIterator<Item = TestClientId>) -> Self {
        ClientFilter::Except(ids.into_iter().collect())
    }

    fn contains(&self, id: TestClientId) -> bool {
        match self {
            ClientFilter::All => true,
            ClientFilter::Only(set) => set.contains(&id),
            ClientFilter::Except(set) => !set.contains(&id),
        }
    }
}

struct World {
    server: Server,
    // Note. Not using `HashMap<server::ClientId, Client>`, because `ClientId`s are meant
    //   to be recyclable and we don't want to reuse IDs in tests.
    clients: Vec<Client>,
}

impl World {
    fn new() -> Self { World { server: Server::new(), clients: vec![] } }

    fn set_time(&mut self, time: Duration) { self.server.set_time(time); }

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
        client.connect(&mut self.server, None);
        self.clients.push(client);
        idx
    }
    fn new_client_registered_user(&mut self, user_name: &str) -> TestClientId {
        let idx = TestClientId(self.clients.len());
        let mut client = Client::new();
        let session_id = self.server.signin_user(user_name);
        client.connect(&mut self.server, Some(session_id));
        self.clients.push(client);
        idx
    }
    fn new_clients<const NUM: usize>(&mut self) -> [TestClientId; NUM] {
        std::array::from_fn(|_| self.new_client())
    }
    fn disconnect_client(&mut self, client_id: TestClientId) {
        let client = &mut self.clients[client_id.0];
        self.server.clients.lock().unwrap().remove_client(client.id.unwrap()).unwrap();
    }
    fn reconnect_client(&mut self, client_id: TestClientId) {
        let client = &mut self.clients[client_id.0];
        self.server.clients.lock().unwrap().remove_client(client.id.unwrap()).unwrap();
        client.connect(&mut self.server, None);
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

        self.join_and_set_team(cl2, &mtch, "p2", Team::Red);
        self.join_and_set_team(cl3, &mtch, "p3", Team::Blue);
        self.join_and_set_team(cl4, &mtch, "p4", Team::Blue);
        self.process_all_events();

        self.new_game_with_default_board_assignment(mtch.clone(), cl1, cl2, cl3, cl4);
        (mtch, cl1, cl2, cl3, cl4)
    }
    fn new_game_with_default_board_assignment(
        &mut self, mtch: String, cl1: TestClientId, cl2: TestClientId, cl3: TestClientId,
        cl4: TestClientId,
    ) {
        self.server.state.TEST_override_board_assignment(mtch, vec![
            single_player("p1", envoy!(White A)), // Red team
            single_player("p2", envoy!(Black B)), // Red team
            single_player("p3", envoy!(Black A)), // Blue team
            single_player("p4", envoy!(White B)), // Blue team
        ]);
        for cl in [cl1, cl2, cl3, cl4].iter() {
            self[*cl].state.set_ready(true);
        }
        self.process_all_events();
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
    fn process_events_from_clients(&mut self, filter: &ClientFilter) -> bool {
        let mut something_changed = false;
        for (id, client) in self.clients.iter_mut().enumerate() {
            if !filter.contains(TestClientId(id)) {
                continue;
            }
            if client.process_outgoing_events(&mut self.server) {
                something_changed = true;
            }
        }
        something_changed
    }
    fn process_events_to_clients(&mut self, filter: &ClientFilter) -> bool {
        let mut something_changed = false;
        for (id, client) in self.clients.iter_mut().enumerate() {
            if !filter.contains(TestClientId(id)) {
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
    fn process_events_for_clients(&mut self, filter: &ClientFilter) {
        let mut something_changed = true;
        while something_changed {
            something_changed = false;
            if self.process_events_from_clients(filter) {
                something_changed = true;
            }
            if self.process_events_to_clients(filter) {
                something_changed = true;
            }
        }
    }
    fn process_all_events(&mut self) {
        self.server.tick();
        self.process_events_for_clients(&ClientFilter::all());
    }

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

    fn replay_three_repetition_draw(&mut self, white_id: TestClientId, black_id: TestClientId) {
        self[white_id].make_turn("Nc3").unwrap();
        self.process_all_events();
        self[black_id].make_turn("Nc6").unwrap();
        self.process_all_events();
        self[white_id].make_turn("Nb1").unwrap();
        self.process_all_events();
        self[black_id].make_turn("Nb8").unwrap();
        self.process_all_events();
        self[white_id].make_turn("Nc3").unwrap();
        self.process_all_events();
        self[black_id].make_turn("Nc6").unwrap();
        self.process_all_events();
        self[white_id].make_turn("Nb1").unwrap();
        self.process_all_events();
        self[black_id].make_turn("Nb8").unwrap();
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

#[test]
fn score_valid() {
    let mut world = World::new();
    let (mtch, cl1, cl2, cl3, cl4) = world.default_clients();
    world.replay_white_checkmates_black(cl1, cl3);
    world.new_game_with_default_board_assignment(mtch, cl1, cl2, cl3, cl4);
    world.replay_three_repetition_draw(cl1, cl3);
    let scores = match world[cl1].state.mtch().as_ref().unwrap().scores.as_ref().unwrap() {
        Scores::PerTeam(v) => v,
        _ => panic!("Expected Scores::PerTeam"),
    };
    assert_eq!(scores[Team::Red].as_f64(), 1.5);
    assert_eq!(scores[Team::Blue].as_f64(), 0.5);
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
    world[cl2].state.leave_server();
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

    world[cl2].state.leave_server();
    world[cl3].state.leave_server();
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
        Err(client::EventError::Ignorable(_))
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

    world[cl3].state.leave_server();
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
        Err(client::EventError::Ignorable(_))
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
    world[cl4].state.leave_server();
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
    world[cl4].state.leave_server();
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
    world.process_events_for_clients(&ClientFilter::except([cl3]));
    world[cl4].make_turn("d4").unwrap();
    world.process_events_for_clients(&ClientFilter::except([cl3]));

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
    world.process_events_for_clients(&ClientFilter::except([cl3]));

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
    world.process_events_for_clients(&ClientFilter::except([cl5]));

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
    world.process_events_for_clients(&ClientFilter::except([cl4]));
    world[cl4].state.hot_reconnect();
    world.process_all_events();
    assert_eq!(
        world[cl4].alt_game().status(),
        BughouseGameStatus::Victory(Team::Blue, VictoryReason::Resignation)
    );
}

// Regression test: client used to report match id mismatch when trying to join a match on slow
// internet and then trying to join another match before the first request was processed.
#[test]
fn join_match_reconsider() {
    let mut world = World::new();
    let [cl1, cl2, cl3] = world.new_clients();

    let mtch1 = world.new_match(cl1, "p1");
    let mtch2 = world.new_match(cl2, "p2");
    world[cl3].join(&mtch1, "p3");
    world[cl3].join(&mtch2, "p3");
    world.process_all_events();
    assert_eq!(world[cl3].state.match_id(), Some(&mtch2));
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

// Regression test: don't show "0:00" time left for players who haven't lost on time.
#[test]
fn time_forfeit() {
    let mut world = World::new();
    let (_, cl1, _cl2, cl3, cl4) = world.default_clients();

    world[cl1].make_turn("e4").unwrap();
    world.process_all_events();
    world.set_time(Duration::seconds(1));
    world[cl3].make_turn("e5").unwrap();
    world.process_all_events();
    world.set_time(Duration::seconds(3));
    world[cl1].make_turn("d4").unwrap();
    world.process_all_events();

    world.set_time(Duration::seconds(60));
    world[cl4].make_turn("e4").unwrap();
    world.process_all_events();

    assert!(world[cl1].alt_game().status().is_active());
    world.set_time(Duration::seconds(1000));
    world.server.tick();
    world.process_all_events();

    let game = world[cl1].local_game();
    assert_eq!(game.status(), BughouseGameStatus::Victory(Team::Red, VictoryReason::Flag));

    // GameInstant shouldn't matter when checking clocks for a finished game.
    // Improvement potential: Factor out clock showing logic to `Client` and check properly.
    let t = GameInstant::game_start();

    assert!(game.board(A).clock().time_left(Black, t).is_zero());

    // Verify that we've recorded accurate game over time even if flag test was delayed.
    assert!(!game.board(A).clock().time_left(White, t).is_zero());
    assert!(!game.board(B).clock().time_left(Black, t).is_zero());
    assert!(!game.board(B).clock().time_left(White, t).is_zero());
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

// Regression test: web client assumes there is a participant for every score entry; it used to
// panic if an observer with a score left.
#[test]
fn no_score_for_observers() {
    let mut world = World::new();
    let [cl1, cl2, cl3, cl4, cl5] = world.new_clients();

    let mtch = world.new_match(cl1, "p1");

    world.server.state.TEST_override_board_assignment(mtch.clone(), vec![
        single_player("p1", envoy!(White A)),
        single_player("p2", envoy!(Black B)),
        single_player("p4", envoy!(Black A)),
        single_player("p5", envoy!(White B)),
    ]);

    world[cl2].join(&mtch, "p2");
    world[cl3].join(&mtch, "p3");
    world[cl4].join(&mtch, "p4");
    world[cl5].join(&mtch, "p5");
    world.process_all_events();

    world[cl3].state.set_faction(Faction::Observer);
    world.process_all_events();

    for cl in [cl1, cl2, cl4, cl5].iter() {
        world[*cl].state.set_ready(true);
    }
    world.process_all_events();

    world.replay_white_checkmates_black(cl1, cl4);
    let scores = match world[cl1].state.mtch().unwrap().scores.as_ref().unwrap() {
        Scores::PerPlayer(scores) => scores,
        _ => panic!(),
    };
    assert_eq!(
        *scores,
        HashMap::from_iter([
            ("p1".to_owned(), HalfU32::whole(1)),
            ("p2".to_owned(), HalfU32::whole(1)),
            ("p4".to_owned(), HalfU32::whole(0)),
            ("p5".to_owned(), HalfU32::whole(0)),
        ])
    );
}

#[test]
fn shared_wayback() {
    let mut world = World::new();
    let (_, cl1, cl2, cl3, _cl4) = world.default_clients();
    world.replay_white_checkmates_black(cl1, cl3);
    world[cl1].state.set_shared_wayback(true);
    world[cl2].state.set_shared_wayback(true);
    world[cl1].state.wayback_to(WaybackDestination::Index(Some(TurnIndex(3))), None);
    world.process_all_events();
    assert_eq!(world[cl1].alt_game().wayback_state().turn_index(), Some(TurnIndex(3)));
    assert_eq!(world[cl2].alt_game().wayback_state().turn_index(), Some(TurnIndex(3)));
    assert_eq!(world[cl3].alt_game().wayback_state().turn_index(), None);
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

    world[cl1].state.request_export(pgn::BpgnExportFormat::default());
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

#[test]
fn chat_basic() {
    let mut world = World::new();
    let (_, cl1, cl2, cl3, _cl4) = world.default_clients();

    world[cl1].state.send_chat_message("hi all".to_owned(), ChatRecipient::All);
    world.process_all_events();
    world[cl2].state.send_chat_message("hi team".to_owned(), ChatRecipient::Team);
    world.process_all_events();
    world[cl3]
        .state
        .send_chat_message("hi p1".to_owned(), ChatRecipient::Participant("p1".to_owned()));
    world.process_all_events();
    world.replay_white_checkmates_black(cl1, cl3);
    world[cl1].state.send_chat_message("gg".to_owned(), ChatRecipient::All);
    world.process_all_events();

    let over = "Game over! p1 & p2 won: p3 & p4 checkmated.";
    assert_eq!(world[cl1].chat_item_text(), ["hi all", "hi team", "hi p1", over, "gg"]);
    assert_eq!(world[cl2].chat_item_text(), ["hi all", "hi team", over, "gg"]);
    assert_eq!(world[cl3].chat_item_text(), ["hi all", "hi p1", over, "gg"]);
}

#[test]
fn chat_ephemeral_message() {
    let mut world = World::new();
    let (_, cl1, _cl2, _cl3, _cl4) = world.default_clients();

    world[cl1].state.send_chat_message("hi".to_owned(), ChatRecipient::Team);
    world[cl1].state.execute_input("<Release the Kraken!");
    assert_eq!(world[cl1].chat_item_text(), ["hi", "Invalid notation."]);
    world[cl1].state.execute_input("<e4");
    assert_eq!(world[cl1].chat_item_text(), ["hi"]);
}

// All clients should eventually see chat messages in the same order determined by the server.
#[test]
fn chat_message_order() {
    let mut world = World::new();
    let (_, cl1, cl2, _cl3, _cl4) = world.default_clients();

    world[cl1].state.send_chat_message("1-a".to_owned(), ChatRecipient::Team);
    world.process_all_events();
    world[cl2].state.send_chat_message("2-a".to_owned(), ChatRecipient::Team);
    world.process_all_events();
    world[cl1].state.send_chat_message("1-b".to_owned(), ChatRecipient::Team);
    world.process_all_events();

    world[cl1].state.send_chat_message("1-c".to_owned(), ChatRecipient::Team);

    world[cl2].state.send_chat_message("2-b".to_owned(), ChatRecipient::Team);
    world.process_events_for_clients(&ClientFilter::only([cl2]));

    assert_eq!(world[cl1].chat_item_text(), ["1-a", "2-a", "1-b", "1-c"]);
    assert_eq!(world[cl2].chat_item_text(), ["1-a", "2-a", "1-b", "2-b"]);

    world.process_all_events();

    assert_eq!(world[cl1].chat_item_text(), ["1-a", "2-a", "1-b", "2-b", "1-c"]);
    assert_eq!(world[cl2].chat_item_text(), ["1-a", "2-a", "1-b", "2-b", "1-c"]);
}

#[test]
fn chat_reconnect() {
    let mut world = World::new();
    let (mtch, cl1, cl2, cl3, _cl4) = world.default_clients();

    world[cl1].state.send_chat_message("1-a".to_owned(), ChatRecipient::All);
    world.process_all_events();
    world[cl2].state.send_chat_message("2-a".to_owned(), ChatRecipient::Team);
    world.process_all_events();

    world[cl3].state.send_chat_message("3-a".to_owned(), ChatRecipient::All);
    world.process_events_for_clients(&ClientFilter::only([cl3]));
    world.reconnect_client(cl3);
    world[cl3]
        .state
        .send_chat_message("3-b".to_owned(), ChatRecipient::Participant("p1".to_owned()));

    world[cl1].state.send_chat_message("1-b".to_owned(), ChatRecipient::All);
    world.process_events_for_clients(&ClientFilter::except([cl3]));
    world[cl2].state.send_chat_message("2-b".to_owned(), ChatRecipient::Team);
    world.process_events_for_clients(&ClientFilter::except([cl3]));

    world[cl3].state.hot_reconnect();
    world.process_all_events();

    let cl5 = world.new_client();
    world[cl5].join(&mtch, "p5");
    world.process_all_events();

    assert_eq!(world[cl1].chat_item_text(), ["1-a", "2-a", "3-a", "1-b", "2-b", "3-b"]);
    assert_eq!(world[cl2].chat_item_text(), ["1-a", "2-a", "3-a", "1-b", "2-b"]);
    assert_eq!(world[cl3].chat_item_text(), ["1-a", "3-a", "1-b", "3-b"]);
    assert_eq!(world[cl5].chat_item_text(), ["1-a", "3-a", "1-b"]);
}

// In dynamic teams mode team chat should be visible to the players who were in sender's team at
// the time of sending the message.
#[test]
fn team_chat_dynamic_teams() {
    let mut world = World::new();
    let [cl1, cl2, cl3, cl4, cl5] = world.new_clients();

    let mtch = world.new_match(cl1, "p1");
    world.process_all_events();
    world[cl2].join(&mtch, "p2");
    world[cl3].join(&mtch, "p3");
    world[cl4].join(&mtch, "p4");
    world[cl5].join(&mtch, "p5");
    world.process_all_events();

    world.server.state.TEST_override_board_assignment(mtch.clone(), vec![
        single_player("p1", envoy!(White A)),
        single_player("p2", envoy!(Black B)),
        single_player("p4", envoy!(Black A)),
        single_player("p5", envoy!(White B)),
    ]);
    for cl in [cl1, cl2, cl3, cl4, cl5].iter() {
        world[*cl].state.set_ready(true);
    }
    world.process_all_events();

    world[cl1].state.send_chat_message("first".to_owned(), ChatRecipient::Team);
    world.process_all_events();
    world[cl1].state.resign();
    world.process_all_events();

    world.server.state.TEST_override_board_assignment(mtch.clone(), vec![
        single_player("p1", envoy!(White A)),
        single_player("p3", envoy!(Black B)),
        single_player("p4", envoy!(Black A)),
        single_player("p5", envoy!(White B)),
    ]);
    for cl in [cl1, cl2, cl3, cl4, cl5].iter() {
        world[*cl].state.set_ready(true);
    }
    world.process_all_events();

    world[cl1].state.send_chat_message("second".to_owned(), ChatRecipient::Team);
    world.process_all_events();

    let over = "Game over! p4 & p5 won: p1 & p2 resigned.";
    assert_eq!(world[cl1].chat_item_text(), ["first", over, "second"]);
    assert_eq!(world[cl2].chat_item_text(), ["first", over]);
    assert_eq!(world[cl3].chat_item_text(), [over, "second"]);
    assert_eq!(world[cl4].chat_item_text(), [over]);
    assert_eq!(world[cl5].chat_item_text(), [over]);

    world[cl2].state.leave_server();
    world[cl3].state.leave_server();
    world.process_all_events();

    let cl2_new = world.new_client();
    world[cl2_new].join(&mtch, "p2");
    let cl3_new = world.new_client();
    world[cl3_new].join(&mtch, "p3");
    world.process_all_events();

    assert_eq!(world[cl2_new].chat_item_text(), ["first", over]);
    assert_eq!(world[cl3_new].chat_item_text(), [over, "second"]);
}

// In fixed teams mode the message is sent to the entire team, regardless of who is currently
// playing.
#[test]
fn team_chat_fixed_teams() {
    let mut world = World::new();
    let [cl1, cl2, cl3, cl4, cl5] = world.new_clients();

    let mtch = world.new_match(cl1, "p1");
    world[cl1].state.set_faction(Faction::Fixed(Team::Red));
    world.process_all_events();
    world.join_and_set_team(cl2, &mtch, "p2", Team::Red);
    world.join_and_set_team(cl3, &mtch, "p3", Team::Red);
    world.join_and_set_team(cl4, &mtch, "p4", Team::Blue);
    world.join_and_set_team(cl5, &mtch, "p5", Team::Blue);
    world.process_all_events();

    world.server.state.TEST_override_board_assignment(mtch.clone(), vec![
        single_player("p1", envoy!(White A)),
        single_player("p2", envoy!(Black B)),
        single_player("p4", envoy!(Black A)),
        single_player("p5", envoy!(White B)),
    ]);
    for cl in [cl1, cl2, cl3, cl4, cl5].iter() {
        world[*cl].state.set_ready(true);
    }
    world.process_all_events();

    world[cl1].state.send_chat_message("first".to_owned(), ChatRecipient::Team);
    world.process_all_events();
    world[cl1].state.resign();
    world.process_all_events();

    world.server.state.TEST_override_board_assignment(mtch.clone(), vec![
        single_player("p1", envoy!(White A)),
        single_player("p3", envoy!(Black B)),
        single_player("p4", envoy!(Black A)),
        single_player("p5", envoy!(White B)),
    ]);
    for cl in [cl1, cl2, cl3, cl4, cl5].iter() {
        world[*cl].state.set_ready(true);
    }
    world.process_all_events();

    world[cl1].state.send_chat_message("second".to_owned(), ChatRecipient::Team);
    world.process_all_events();

    let over = "Game over! p4 & p5 won: p1 & p2 resigned.";
    assert_eq!(world[cl1].chat_item_text(), ["first", over, "second"]);
    assert_eq!(world[cl2].chat_item_text(), ["first", over, "second"]);
    assert_eq!(world[cl3].chat_item_text(), ["first", over, "second"]);
    assert_eq!(world[cl4].chat_item_text(), [over]);
    assert_eq!(world[cl5].chat_item_text(), [over]);

    world[cl2].state.leave_server();
    world[cl3].state.leave_server();
    world.process_all_events();

    let cl2_new = world.new_client();
    world[cl2_new].join(&mtch, "p2");
    let cl3_new = world.new_client();
    world[cl3_new].join(&mtch, "p3");
    world.process_all_events();

    assert_eq!(world[cl2_new].chat_item_text(), ["first", over, "second"]);
    assert_eq!(world[cl3_new].chat_item_text(), ["first", over, "second"]);
}

// Regression test: we used to not remove offline participants from the lobby if there was an active
// client with the same participant ID in another match.
#[test]
fn two_matches_same_participant_id() {
    let mut world = World::new();

    let m1_cl1 = world.new_client_registered_user("Alice");
    let m1_cl2 = world.new_client_registered_user("Bob");
    let m1 = world.new_match(m1_cl1, "Alice");
    world[m1_cl2].join(&m1, "Bob");
    world.process_all_events();

    let m2_cl = world.new_client_registered_user("Alice");
    world.new_match(m2_cl, "Alice");

    world.disconnect_client(m1_cl1);
    world.process_all_events();
    assert_eq!(world[m1_cl2].state.mtch().unwrap().participants.len(), 1);
}

// It's ok for a registered user to reconnect at any point kicking out the old client: we know it's
// the same person.
#[test]
fn registered_user_reconnect() {
    let mut world = World::new();

    let cl_old = world.new_client_registered_user("Alice");
    let mtch = world.new_match(cl_old, "Alice");

    let cl_new = world.new_client_registered_user("Alice");
    world[cl_new].join(&mtch, "Alice");

    world.process_events_for(cl_new).unwrap();
    assert!(matches!(
        world.process_events_for(cl_old),
        Err(client::EventError::KickedFromMatch(_))
    ));
    world.process_all_events();
}

// Register user can kick a guest user with the same name.
#[test]
fn registered_user_after_guest_user() {
    let mut world = World::new();

    let cl_old = world.new_client();
    let mtch = world.new_match(cl_old, "Alice");

    let cl_new = world.new_client_registered_user("Alice");
    world[cl_new].join(&mtch, "Alice");

    world.process_events_for(cl_new).unwrap();
    assert!(matches!(
        world.process_events_for(cl_old),
        Err(client::EventError::KickedFromMatch(_))
    ));
    world.process_all_events();
}

// Guest user cannot connect if there is a registered user with the same name.
#[test]
fn guest_user_after_registered_user() {
    let mut world = World::new();

    let cl_old = world.new_client_registered_user("Alice");
    let mtch = world.new_match(cl_old, "Alice");

    let cl_new = world.new_client();
    world[cl_new].join(&mtch, "Alice");

    assert!(matches!(
        world.process_events_for(cl_new),
        Err(client::EventError::Ignorable(_))
    ));
    world.process_all_events();
}

// Guest user cannot kick out other guest user if their client is still alive. Of course, usually
// this would be ok. Most likely this is the same person trying to reconnect before the game
// realized that there was a problem with the old connection. However we cannot allow this, because
// it would allow others to steal guest user identities.
#[test]
fn guest_user_reconnect_early() {
    let mut world = World::new();
    let [cl_old, cl_new] = world.new_clients();

    let mtch = world.new_match(cl_old, "Alice");
    world[cl_new].join(&mtch, "Alice");

    assert!(matches!(
        world.process_events_for(cl_new),
        Err(client::EventError::Ignorable(_))
    ));
    world.process_all_events();
}

// Guest user can reconnect if the old client is dead.
#[test]
fn guest_user_reconnect_ok() {
    let mut world = World::new();
    let [cl_old, cl_new] = world.new_clients();

    let mtch = world.new_match(cl_old, "Alice");
    world.disconnect_client(cl_old);
    world.process_all_events();

    world[cl_new].join(&mtch, "Alice");
    world.process_all_events();
}
