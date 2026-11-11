const { helper } = require("./helper.cjs");

function local() {}

module.exports = { local, helper };

module.exports.method = function () {};
