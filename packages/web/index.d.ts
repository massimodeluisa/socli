export interface ReaderFrame {
  frame: Uint8Array
  text: string
  cursor: { col: number; row: number } | null
  clipboard: string
  open: string
  pictures: Array<{
    url: string
    col: number
    row: number
    cols: number
    rows: number
    avatar: boolean
    square: boolean
    png?: ArrayBuffer
  }>
}

export interface ReaderOptions {
  wasmUrl?: string | URL
  workerUrl?: string | URL
  pageUrl?: string | URL
  relayUrl?: string | URL | null
  config?: string
  configStore?: {
    load(): string | Promise<string>
    save(text: string): void | Promise<void>
  }
  oauth?: { code: string; state: string } | null
  onFrame?(frame: ReaderFrame): void
  onError?(error: Error): void
}

export interface MountedReaderOptions extends ReaderOptions {
  theme?: 'auto' | 'dark' | 'light'
  styleUrl?: string | URL
  fonts?: boolean
  autofocus?: boolean
  onOpenUrl?(url: string): void | Promise<void>
  onClipboard?(text: string): void | Promise<void>
}

export interface MountedReader {
  readonly text: string
  key(name: string): Promise<ReaderFrame>
  write(bytes: Uint8Array): Promise<ReaderFrame>
  paste(text: string): Promise<ReaderFrame>
  resize(): void
  focus(): void
  flushConfig(): Promise<void>
  setTheme(theme: 'auto' | 'dark' | 'light'): Promise<ReaderFrame>
  dispose(): void
}

export interface Reader {
  readonly frame: ReaderFrame
  readonly text: string
  write(bytes: Uint8Array): Promise<ReaderFrame>
  paste(text: string): Promise<ReaderFrame>
  pointer(kind: number, col: number, row: number, button?: number): Promise<ReaderFrame>
  resize(cols: number, rows: number, ratio?: number): Promise<ReaderFrame>
  tick(): Promise<ReaderFrame>
  flushConfig(): Promise<void>
  dispose(): void
}

export const wasmUrl: URL
export const workerUrl: URL
export const styleUrl: URL
export function instantiate(imports: WebAssembly.Imports, url?: string | URL): Promise<WebAssembly.WebAssemblyInstantiatedSource>
export function createReader(options?: ReaderOptions & { cols?: number; rows?: number; dark?: boolean }): Promise<Reader>
export function mountReader(container: HTMLElement, options?: MountedReaderOptions): Promise<MountedReader>
