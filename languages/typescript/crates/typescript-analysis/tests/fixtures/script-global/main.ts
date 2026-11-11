// No import or export: this file is a script, so its functions are globals and merge with the DOM library's declarations of the same name.
function fetch(): number {
  return 1;
}

function open(): number {
  return 2;
}

// Declared here and in the DOM library, and the project's declaration has no body: nothing to prefer, so both stay.
declare function alert(message: string): void;

function caller() {
  fetch();
  open();
  alert("both");
}
