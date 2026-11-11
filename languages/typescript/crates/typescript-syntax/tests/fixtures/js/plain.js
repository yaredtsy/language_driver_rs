function legacy(a, b) {
  return a + b;
}

var oldStyle = function () {};

const modern = () => {};

class Modern {
  method() {}
  static create() {}
}

Legacy.prototype.proto = function () {};

module.exports = function () {};

exports.helper = function helper() {};

(function iife() {
  function insideIife() {}
})();
