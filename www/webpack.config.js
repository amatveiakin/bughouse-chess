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
    ],
  },
  experiments: {
    asyncWebAssembly: true,
  },
  devServer: {
    proxy: {
      "/dyn": "http://127.0.0.1:14361",
      "/auth": "http://127.0.0.1:14361",
    },
  },
};
