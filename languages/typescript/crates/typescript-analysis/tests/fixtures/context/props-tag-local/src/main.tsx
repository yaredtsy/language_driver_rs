function a() {}
function b() {}
function A() { a(); return null; }
function B() { b(); return null; }
function main(flag: boolean) { const C = flag ? A : B; return <C />; }
