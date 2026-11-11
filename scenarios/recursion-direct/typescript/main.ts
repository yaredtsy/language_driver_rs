// A module, as TypeScript files are: a script would merge its functions with the DOM's globals (`fetch`).
export {};

function helper() {
  return 1;
}

function countdown(n: number) {
  if (n > 0) {
    countdown(n - 1);
  }
  helper();
}

function main() {
  countdown(3);
}
