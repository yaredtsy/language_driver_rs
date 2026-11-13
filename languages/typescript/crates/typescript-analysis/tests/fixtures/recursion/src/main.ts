// Cycles of every shape, for the stitching test: a request rooted at a def that a bounded tree cut does not know what is above it.

export function main(): void {
  ping(3);
  ring1();
  makeTree();
  visit();
}

// two functions that call each other
export function ping(n: number): number {
  return n > 0 ? pong(n - 1) : 0;
}

export function pong(n: number): number {
  return ping(n - 1) + leaf();
}

function leaf(): number {
  return 1;
}

// three, and the last calls two of those above it
export function ring1(): void {
  ring2();
  leaf();
}

function ring2(): void {
  ring3();
}

function ring3(): void {
  ring1();
  ring2();
}

// a construction that is on its own path
export class Tree {
  constructor() {
    grow();
  }
}

function grow(): Tree {
  return new Tree();
}

export function makeTree(): Tree {
  return new Tree();
}

// a def that hands itself over
function visit(): void {
  schedule(visit);
}

function schedule(callback: () => void): void {
  callback();
}
