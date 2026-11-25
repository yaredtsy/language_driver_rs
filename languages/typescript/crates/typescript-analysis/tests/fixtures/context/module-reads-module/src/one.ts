import { second } from "./two";
export function a() {}
export const first: () => void = second ?? a;
