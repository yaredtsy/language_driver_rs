class Saver { save() { base(); } }
function base() {}
function run(s: Saver) { s.save(); }
function main() { run(new Saver()); }
