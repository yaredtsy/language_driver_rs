function read() {
  return 0;
}
function write(n: number) {}
function other() {}

class Box {
  plain = 0;
  get value() {
    return read();
  }
  set value(n: number) {
    write(n);
  }
  get readOnly() {
    return 1;
  }
  get handler() {
    return () => {};
  }
  get inner(): Box {
    return this;
  }
  static get count() {
    return other();
  }
  get #secret() {
    return 1;
  }
  peek() {
    return this.#secret;
  }
  method() {}
  viaThis() {
    return this.value;
  }
}
class Sub extends Box {
  viaSuper() {
    return super.value;
  }
}
class Other {
  get value() {
    return 2;
  }
}
class Flat {
  value = 3;
}
declare const box: Box;
declare const key: string;
declare const remote: Remote;
declare const either: Box | Other;
declare const mixed: Box | Flat;
const literal = {
  get lit() {
    return read();
  },
  set lit(n: number) {},
};

// the getter for a read, the setter for a write, both for `+=`, `++` and `||=`
export function reads() {
  return box.value;
}
export function writes() {
  box.value = 1;
}
export function compound() {
  box.value += 1;
}
export function increment() {
  box.value++;
  --box.value;
}
export function logicalAssign() {
  box.value ||= 1;
}
export function inForOf() {
  for (box.value of [1]) {
  }
}
export function inDestructuringTarget() {
  [box.value] = [1];
}
// through assertions and parentheses
export function wrapped() {
  (box.value as number) = 1;
  (box.value!) = 2;
}
// a property access that is already a callee is the callee's site only
export function callee() {
  box.method();
  box.handler();
}
// each access of a chain is its own read
export function chain() {
  return box.inner.value;
}
// no accessor, no site: a field, a method that is not called, a deleted property, a computed key
export function none() {
  box.plain = 1;
  const m = box.method;
  delete box.value;
  return box[key];
}
export function elementWithLiteral() {
  return box["value"];
}
export function staticGetter() {
  return Box.count;
}
export function privateGetter(b: Box) {
  return b.peek();
}
export function inherited(s: Sub) {
  return s.viaSuper();
}
export function accessorInObjectLiteral() {
  literal.lit = literal.lit;
}
// a union: the getter of every member that has one; a member that is a plain field runs nothing
export function unionOfAccessors() {
  return either.value;
}
export function unionWithAField() {
  return mixed.value;
}
// declared in a `.d.ts` of the repo
export function declaredAccessor() {
  remote.size += 1;
}
// a computed key that is itself an accessor read: the key is the site, the element access runs nothing
export function keyThatIsAnAccessor() {
  return (box as any)[remote.size];
}

// a getter read by destructuring runs, and no property access is there to be its site: counted, not drawn
export function destructuredGetter() {
  const { value } = box;
  return value;
}
export function destructuredNested() {
  const {
    inner: { value },
  } = box;
  return value;
}
export function destructuredExternal() {
  const { size } = remote;
  return size;
}
export function destructuredField() {
  const { plain } = box;
  return plain;
}
export function destructuredRest() {
  const { ...rest } = box;
  return rest;
}
