function a() {
  function b() {
    class C {
      m() {
        function d() {}
        const e = () => {
          class F {}
        };
      }
    }
  }
}

class Outer {
  method() {
    return class {
      inner() {}
    };
  }
  other() {
    const helper = () => {};
  }
}

const holder = {
  method() {
    function insideObjectMethod() {}
  },
};

if (flag) {
  function inIf() {
    function insideInIf() {}
  }
} else {
  function inElse() {}
}

try {
  function inTry() {}
} catch (e) {
  function inCatch() {}
}

for (const x of xs) {
  function inLoop() {}
}

switch (k) {
  case 1:
    function inCase() {}
}
