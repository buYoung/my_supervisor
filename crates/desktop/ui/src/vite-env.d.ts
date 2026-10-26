/// <reference types="vite/client" />

interface ImportMetaEnv {
  readonly VITE_NATIVE_PROXY?: boolean;
  readonly VITE_API_BASE_URL?: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}
