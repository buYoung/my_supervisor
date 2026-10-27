// Vite aliases only the E2E build's module import to this live binding. Native
// Tauri globals stay untouched; restoration returns the real exported invoke.
import * as real from "../../node_modules/@tauri-apps/api/core.js";
export * from "../../node_modules/@tauri-apps/api/core.js";

export let invoke: typeof real.invoke = real.invoke;
export function enableMockRouting() {
  invoke = <T>(command: string, args?: real.InvokeArgs, options?: real.InvokeOptions): Promise<T> => {
    const mock = (window as unknown as {__wdio_mocks__?: Record<string, (args?: real.InvokeArgs) => Promise<T>>}).__wdio_mocks__?.[command];
    return mock ? Promise.resolve().then(() => mock(args)) : real.invoke<T>(command, args, options);
  };
}
export function restoreInvoke() {invoke = real.invoke;}
export function isInvokeRestored() {return invoke === real.invoke;}
