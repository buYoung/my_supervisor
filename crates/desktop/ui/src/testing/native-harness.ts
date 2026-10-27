// Loaded only by Vite's explicit desktop-e2e mode; absent from ordinary output.
import "@wdio/tauri-plugin";
import {getCurrentWebviewWindow} from "@tauri-apps/api/webviewWindow";
import {enableMockRouting, restoreInvoke, isInvokeRestored} from "./tauri-core";
import {getOperationsClient} from "../services";
import * as fixtures from "./fixtures";

const host = window as unknown as {
  __wdio_mocks__?: Record<string, (args?: Record<string, unknown>) => Promise<unknown>>;
  __MSV_E2E__?: typeof harness;
};

const nativeInputEvents: Record<string, unknown>[] = [];
let nativeKeyEventCount = 0;
document.addEventListener("keydown", event => {if(event.isTrusted)nativeKeyEventCount++;}, true);
const clipboard = {value: ""};
const isOsObservation = import.meta.env.VITE_E2E_OS_OBSERVE === true;
if (!isOsObservation) Object.defineProperty(navigator,"clipboard",{configurable:true,value:{
  writeText: async (value:string) => {clipboard.value=value;},
  readText: async () => clipboard.value,
}});
let isObservingNativeInput = false;
const harness = {
  get nativeKeyEventCount() {return nativeKeyEventCount;},
  clipboard,
  nativeInputEvents,
  beginNativeInputObservation() {
    nativeInputEvents.length = 0;
    if (isObservingNativeInput) return;
    isObservingNativeInput = true;
    for (const type of ["compositionstart", "compositionupdate", "compositionend", "beforeinput", "input", "keydown", "keyup"]) {
      document.addEventListener(type, event => {
        // Autonomous runs never record physical keyboard/input content.
        if (event.isTrusted && !isOsObservation) return;
        const input = event as InputEvent;
        const key = event as KeyboardEvent;
        nativeInputEvents.push({type, data: input.data, isTrusted: event.isTrusted, isComposing: input.isComposing ?? key.isComposing, inputType: input.inputType, key: key.key, code: key.code, keyCode: key.keyCode, value: (event.target as HTMLInputElement)?.value, rowCount: document.querySelectorAll(".repeated-row").length, focused: document.activeElement?.getAttribute("aria-label")});
      }, true);
    }
  },
  revision: import.meta.env.VITE_E2E_REVISION as string,
  sourceSha256: import.meta.env.VITE_E2E_SOURCE_SHA256 as string,
  windowLabel: getCurrentWebviewWindow().label,
  isWindowVisible: () => getCurrentWebviewWindow().isVisible(),
  client: getOperationsClient(),
  ...fixtures,
  enableMockRouting,
  restoreInvoke,
  isInvokeRestored() {
    return isInvokeRestored() && Object.keys(host.__wdio_mocks__ ?? {}).length === 0;
  },
};
host.__MSV_E2E__ = harness;

declare global {
  interface Window {
    __MSV_E2E__: typeof harness;
  }
}
