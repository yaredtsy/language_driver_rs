// Functions a library made (row `library-made`, D56): named after the call as written and the path to the name.
import { useState, useMutation, useNavigate, create } from "hooks";

declare function localMaker(): () => void;

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
}
