const CopyWebpackPlugin = require("copy-webpack-plugin");
const path = require('path');

module.exports = {
  entry: "./bootstrap.js",
  output: {
    path: path.resolve(__dirname, "dist"),
    filename: "bootstrap.js"
  },
  mode: "development",
  plugins: [
    new CopyWebpackPlugin({ patterns: ['*.html'] }),
    new CopyWebpackPlugin({
      patterns: [
        { from: "./node_modules/@jstable/jstable/dist/jstable.min.js",
          to: "jstable.js"
        }
      ]
    })
  ],
  module: {
    rules: [
      {
        test: /\.css$/,
        use: [
          'style-loader',
          'css-loader'
        ]
      },
      {
        test: /\.(png|svg|jpg|jpeg|gif|ogg)$/i,
        type: 'asset/resource',
      },
    ]
  },
  experiments: {
    asyncWebAssembly: true,
  },
  devServer: {
    proxy: {
      '/dyn': 'http://127.0.0.1:14361',
      '/auth': 'http://127.0.0.1:14361',
    },
  },
};
