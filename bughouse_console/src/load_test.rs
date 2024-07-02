use std::collections::HashMap;
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use std::{io, thread};

use bughouse_chess::board::{Board, Turn, TurnInput, TurnMode};
use bughouse_chess::client::{ClientState, GameState};
use bughouse_chess::display::get_display_board_index;
use bughouse_chess::game::TOTAL_ENVOYS;
use bughouse_chess::meter::{MeterStats, METER_SIGNIFICANT_DIGITS};
use bughouse_chess::rules::{ChessRules, MatchRules, Rules};
use bughouse_chess::server::MATCH_ID_ALPHABET;
use hdrhistogram::Histogram;
use instant::Instant;
use itertools::Itertools;
use rand::seq::SliceRandom;
use rand::Rng;
use url::Url;

use crate::network;


pub struct LoadTestConfig {
    pub server_address: String,
    pub num_matches: usize,
}

struct Client {
    state: ClientState,
    socket: tungstenite::WebSocket<tungstenite::stream::MaybeTlsStream<TcpStream>>,
    aggregated_meter_box: Arc<Mutex<AMeterBox>>,
}

#[derive(Clone, Debug, Default)]
pub struct AMeterBox {
    meters: HashMap<String, AMeter>,
}

impl AMeterBox {
    pub fn new() -> Self { AMeterBox { meters: HashMap::new() } }
    pub fn read_stats(&self) -> HashMap<String, MeterStats> {
        self.meters.iter().map(|(name, meter)| (name.clone(), meter.stats())).collect()
    }

    pub fn add(&mut self, other: &HashMap<String, Histogram<u64>>) {
        for (name, meter) in other {
            self.meters.entry(name.clone()).or_insert_with(AMeter::new).add(meter);
        }
    }
}

#[derive(Clone, Debug)]
pub struct AMeter {
    histogram: Arc<Mutex<Histogram<u64>>>,
}

impl AMeter {
    fn new() -> Self {
        AMeter {
            histogram: Arc::new(Mutex::new(Histogram::new(METER_SIGNIFICANT_DIGITS).unwrap())),
        }
    }

    pub fn add(&mut self, other: &Histogram<u64>) {
        let mut histogram = self.histogram.lock().unwrap();
        histogram.add(other).unwrap();
    }

    fn stats(&self) -> MeterStats {
        let histogram = self.histogram.lock().unwrap();
        MeterStats {
            p50: histogram.value_at_quantile(0.5),
            p90: histogram.value_at_quantile(0.9),
            p99: histogram.value_at_quantile(0.99),
            num_values: histogram.len(),
        }
    }
}

impl Client {
    fn new(server_address: &str, aggregated_meter_box: Arc<Mutex<AMeterBox>>) -> io::Result<Self> {
        let server_addr;
        let ws_request;
        if server_address == "localhost" {
            server_addr = (server_address, network::PORT).to_socket_addrs().unwrap().collect_vec();
            ws_request = Url::parse(&format!("ws://{}", server_address)).unwrap();
        } else {
            server_addr = (server_address, 443).to_socket_addrs().unwrap().collect_vec();
            ws_request = Url::parse(&format!("wss://{}/ws", server_address)).unwrap();
        }
        let stream = TcpStream::connect(&server_addr[..])?;
        let (socket, _) = tungstenite::client_tls(ws_request, stream).unwrap();

        let user_agent = "Loadtest".to_owned();
        let time_zone = "?".to_owned();
        let mut state = ClientState::new(user_agent, time_zone);
        state.disable_performance_reporting();
        Ok(Client { state, socket, aggregated_meter_box })
    }

    // Send all outgoing event and wait for one event from the server.
    fn execute_network_round(&mut self) {
        while let Some(event) = self.state.next_outgoing_event() {
            network::write_obj(&mut self.socket, &event).unwrap();
        }
        let event = network::read_obj(&mut self.socket).unwrap();
        self.state.process_server_event(event).unwrap();
    }

    fn ensure_active_game(&mut self) {
        while !self.state.mtch().map_or(false, |m| m.has_active_game()) {
            if self.state.is_ready() == Some(false) {
                self.state.set_ready(true);
            }
            self.execute_network_round();
        }
    }

    fn run_one_game(&mut self) {
        let rng = &mut rand::thread_rng();
        let mut random_delay = || Duration::from_millis(rng.gen_range(100..=300));
        let mut next_action = Instant::now() + random_delay();
        loop {
            let GameState { alt_game, .. } = self.state.game_state().unwrap();
            if !alt_game.is_active() {
                break;
            }
            let now = Instant::now();
            let my_envoy = alt_game.my_id().as_player().unwrap().as_single_player().unwrap();
            let local_game = alt_game.local_game().clone();
            if !local_game.is_envoy_active(my_envoy) {
                self.execute_network_round();
                continue;
            }
            thread::sleep_until(next_action);
            let board_idx = my_envoy.board_idx;
            let board = local_game.board(board_idx);
            let display_board_idx = get_display_board_index(board_idx, alt_game.perspective());
            let turn = get_random_turn(board);
            if let Some(turn) = turn {
                self.state.make_turn(display_board_idx, TurnInput::Explicit(turn)).unwrap();
            } else {
                self.state.resign();
            }
            next_action = now + random_delay();
        }
    }

    fn aggregate_metrics(&mut self) {
        self.aggregated_meter_box
            .lock()
            .unwrap()
            .add(&self.state.consume_meter_histograms());
    }

    fn run_main_loop(&mut self) {
        loop {
            self.ensure_active_game();
            self.run_one_game();
            self.aggregate_metrics();
        }
    }
}

fn get_random_turn(board: &Board) -> Option<Turn> {
    let keep_legal = |turns: Vec<_>| {
        turns
            .into_iter()
            .filter(|&t| board.is_turn_legal(t, TurnMode::InOrder))
            .collect_vec()
    };
    const DROP_PROBABILITY: f64 = 0.3;
    let rng = &mut rand::thread_rng();
    let moves = keep_legal(board.potential_moves());
    let drops = keep_legal(board.potential_drops());
    if moves.is_empty() && drops.is_empty() {
        None
    } else if moves.is_empty() {
        Some(*drops.choose(rng).unwrap())
    } else if drops.is_empty() {
        Some(*moves.choose(rng).unwrap())
    } else {
        if rng.gen_bool(DROP_PROBABILITY) {
            Some(*drops.choose(rng).unwrap())
        } else {
            Some(*moves.choose(rng).unwrap())
        }
    }
}

fn run_match(
    server_address: &str, test_id: &str, match_index: usize,
    aggregated_meter_box: Arc<Mutex<AMeterBox>>,
) {
    let server_address = server_address.to_owned();
    let test_id = test_id.to_owned();
    let aggregated_meter_box_copy = Arc::clone(&aggregated_meter_box);

    thread::spawn(move || {
        let rules = Rules {
            match_rules: MatchRules::unrated_public(),
            chess_rules: ChessRules::bughouse_international5(),
        };

        let mut first_client = Client::new(&server_address, aggregated_meter_box).unwrap();
        first_client
            .state
            .set_guest_player_name(Some(player_name(&test_id, match_index, 0)));
        first_client.state.new_match(rules);
        while first_client.state.match_id().is_none() {
            first_client.execute_network_round();
        }
        let match_id = first_client.state.match_id().unwrap();
        println!("Starting match {match_id}...");

        for player_index in 1..TOTAL_ENVOYS {
            let aggregated_meter_box = Arc::clone(&aggregated_meter_box_copy);
            let server_address = server_address.to_owned();
            let test_id = test_id.to_owned();
            let match_id = match_id.to_owned();
            thread::spawn(move || {
                let mut client = Client::new(&server_address, aggregated_meter_box).unwrap();
                client.state.set_guest_player_name(Some(player_name(
                    &test_id,
                    match_index,
                    player_index,
                )));
                client.state.join(match_id.clone());
                client.run_main_loop();
            });
        }

        // Implement a barrier: the first player wouldn't set ready until all players have joined.
        while first_client.state.mtch().unwrap().participants.len() < TOTAL_ENVOYS {
            first_client.execute_network_round();
        }
        first_client.run_main_loop();
    });
}

fn stat_to_str(stat: Option<MeterStats>) -> String {
    stat.map_or("?".to_owned(), |s| s.to_string())
}

fn monitor(aggregated_meter_box: Arc<Mutex<AMeterBox>>) -> ! {
    loop {
        thread::sleep(Duration::from_secs(10));
        let mut stats = aggregated_meter_box.lock().unwrap().read_stats();
        let turn_confirmation = stats.remove("turn_confirmation");
        // Should we show ping as well?
        println!("turn_confirmation: {}", stat_to_str(turn_confirmation));
    }
}

fn player_name(test_id: &str, match_index: usize, player_index: usize) -> String {
    format!("loadtest-{test_id}-{match_index:04}-{player_index}")
}

pub fn run(config: LoadTestConfig) -> io::Result<()> {
    let rng = &mut rand::thread_rng();
    let test_id: String = rng
        .sample_iter(rand::distributions::Uniform::from(0..MATCH_ID_ALPHABET.len()))
        .map(|idx| MATCH_ID_ALPHABET[idx])
        .take(3)
        .collect();
    println!("Test ID: {test_id}");

    ctrlc::set_handler(move || std::process::exit(0)).unwrap();

    let aggregated_meter_box = Arc::new(Mutex::new(AMeterBox::new()));
    for match_id in 0..config.num_matches {
        run_match(&config.server_address, &test_id, match_id, Arc::clone(&aggregated_meter_box));
    }
    monitor(aggregated_meter_box);
}
