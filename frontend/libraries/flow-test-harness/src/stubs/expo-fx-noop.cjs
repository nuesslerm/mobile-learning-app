// expo 58 ships its winter polyfills and dev HMR setup as ES modules with extensionless imports and loads them through require(), which node cannot resolve; jsdom has these globals and no metro socket.
module.exports = {};
