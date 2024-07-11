const webpack = require("webpack");
const CopyWebpackPlugin = require("copy-webpack-plugin");
const path = require("path");

module.exports = {
  entry: "./bootstrap.js",
  output: {
    path: path.resolve(__dirname, "dist"),
    filename: "bootstrap.js",
  },
  mode: "development",
  plugins: [
    new CopyWebpackPlugin({
      patterns: [
        "**/index.html",
        "info-page.css",
        "robots.txt",
        "sitemap.txt",
        {
          from: "../assets/logo.svg",
          to: "logo.svg",
        },
        {
          from: "../assets/favicon.png",
          to: "favicon.png",
        },
        {
          from: "./node_modules/@jstable/jstable/dist/jstable.min.js",
          to: "jstable.js",
        },
        {
          from: "./node_modules/fairy-stockfish-nnue.wasm/stockfish.wasm",
          to: "stockfish.wasm",
        },
        {
          from: "./node_modules/fairy-stockfish-nnue.wasm/stockfish.worker.js",
          to: "stockfish.worker.js",
          // HACK. This is a really terrible hack to make Stockfish available to stockfish.worker.js.
          // As far as I can tell, `fairy-stockfish-nnue.wasm` works by spilling the Stockfish object
          // into the global scope, and webpack really doesn't like it when modules try to pollute the
          // global scope. The supposed escape hatches (`expose-loader`, `exports-loader`) didn't work
          // for me (see below). This is the only way I managed to make it work. It's ugly and it has
          // the side effect that `stockfish.js` and `stockfish.worker.js` are now downloaded twice.
          // TODO: Find a proper way to use npm packages communicating via global objects in webpack.
          transform(content) {
            return 'importScripts("./stockfish.js");\n\n' + content.toString();
          },
        },
        {
          // For importScripts trick above.
          from: "./node_modules/fairy-stockfish-nnue.wasm/stockfish.js",
          to: "stockfish.js",
        },
      ],
    }),
  ],
  module: {
    rules: [
      {
        test: /main\.css$/,
        use: ["style-loader", "css-loader"],
      },
      {
        test: /\.(png|svg|jpg|jpeg|gif|ogg)$/i,
        type: "asset/resource",
      },
      // Note. These were attempts to expose Stockfish to stockfish.worker.js, but they didn't work.
      // {
      //   test: require.resolve("fairy-stockfish-nnue.wasm/stockfish.js"),
      //   loader: "expose-loader",
      //   options: {
      //     exposes: ["Stockfish"],
      //   },
      // },
      // {
      //   test: require.resolve("fairy-stockfish-nnue.wasm/stockfish.js"),
      //   loader: "exports-loader",
      //   // options: {
      //   //   exports: "default Stockfish",
      //   // },
      //   options: {
      //     type: "commonjs",
      //     exports: "single Stockfish",
      //   },
      // },
    ],
  },
  // Required to load Fairy-Stockfish. It doesn't need any polyfills, because it detects browser
  // environment automatically and acts accordingly.
  resolve: {
    fallback: {
      fs: false,
      tls: false,
      net: false,
      path: false,
      zlib: false,
      http: false,
      https: false,
      stream: false,
      perf_hooks: false,
      worker_threads: false,
      crypto: false,
    },
  },
  experiments: {
    asyncWebAssembly: true,
  },
  devServer: {
    proxy: {
      "/dyn": "http://127.0.0.1:14361",
      "/auth": "http://127.0.0.1:14361",
    },
    headers: {
      // Required for SharedArrayBuffer used by Fairy-Stockfish.
      "Cross-Origin-Opener-Policy": "same-origin",
      "Cross-Origin-Embedder-Policy": "require-corp",
    },
  },
};
