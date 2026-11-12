// D21 in plain JavaScript: the same rule, no types.
const api = {
  load: () => {},
  save: function () {},
  nested: { deep: async () => {} },
};

let mutable = { a: () => {} };
register({ onSuccess: () => {} });
