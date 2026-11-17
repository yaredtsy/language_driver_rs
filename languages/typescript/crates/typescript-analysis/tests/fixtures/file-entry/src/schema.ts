class Schema {
  refine(check: () => boolean): Schema {
    return this;
  }
}

const schema = {
  object(shape: object): Schema {
    return new Schema();
  },
};

function check(): boolean {
  return true;
}

export const S = schema.object({}).refine(() => check());
