function leaf() {}
function other() {}
async function fetchIt() {
  await leaf();
}
function* gen() {
  yield other();
}
// transparent: the body is walked
export async function run() {
  await fetchIt();
  for (const x of gen()) {
    leaf();
  }
}
