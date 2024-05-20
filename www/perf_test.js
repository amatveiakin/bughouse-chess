// TODO: Proper performance tests. For now the solution is to copy-paste this into index.js

function sleep(time) {
  return new Promise((resolve) => setTimeout(resolve, time));
}

async function run_perf_test() {
  const SLEEP = 50;
  for (let i = 0; i < 10; ++i) {
    wasm_client().click_element(`secondary-p6p4`);
    update();
    await sleep(SLEEP);
    wasm_client().click_element(`secondary-p4p4`);
    update();
    await sleep(SLEEP);
    wasm_client().cancel_preturn("secondary");
    update();
    await sleep(SLEEP);
  }
  execute_input("/perf");
}

// in `execute_input`:
//   case "test": {
//     run_perf_test();
//     break;
//   }
