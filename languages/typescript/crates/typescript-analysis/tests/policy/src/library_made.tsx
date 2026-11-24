// Functions a library made (row `library-made`, D56): named after the call as written and the path to the name.
import { useState, useMutation, useNavigate, create, useCallback, makeStore } from "hooks";

declare function localMaker(): () => void;
declare const handler: () => void;
declare const initial: { go: () => void };
declare const handlers: Map<string, () => void>;

export const useStore = create<{ n: number }>()(() => ({ n: 1 }));

export function library_made() {
  const [open, setOpen] = useState(false);
  setOpen(!open);
  const { mutate } = useMutation();
  mutate();
  const navigate = useNavigate();
  navigate("/");
  useStore();
  let later = useNavigate();
  later("/");
  const { deep: { go } } = useMutation();
  go();
  const made = localMaker();
  made();
  const remembered = useCallback(handler, []);
  remembered();
  const store = makeStore(initial);
  const picked = store((s) => s.go);
  picked();
  const { go: fromState } = store.getState();
  fromState();
  const fromMap = handlers.get("a");
  if (fromMap) fromMap();
}
