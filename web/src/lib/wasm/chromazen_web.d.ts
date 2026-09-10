/* tslint:disable */
/* eslint-disable */

export class WebCanvas {
    private constructor();
    free(): void;
    [Symbol.dispose](): void;
    beginStroke(x: number, y: number, pressure: number, time_ms: number): boolean;
    clear(): boolean;
    static create(element: HTMLCanvasElement, width: number, height: number, scale: number): Promise<WebCanvas>;
    endStroke(): boolean;
    panBy(delta_x: number, delta_y: number): boolean;
    pushStrokeSamples(samples: Float32Array): boolean;
    redo(): boolean;
    render(): boolean;
    resize(width: number, height: number, scale: number): void;
    setBrushSize(size: number): void;
    setColor(red: number, green: number, blue: number): void;
    setTool(tool: number): void;
    undo(): boolean;
    zoomAt(factor: number, x: number, y: number): boolean;
}

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly __wbg_webcanvas_free: (a: number, b: number) => void;
    readonly webcanvas_beginStroke: (a: number, b: number, c: number, d: number, e: number) => number;
    readonly webcanvas_clear: (a: number) => number;
    readonly webcanvas_create: (a: any, b: number, c: number, d: number) => any;
    readonly webcanvas_endStroke: (a: number) => number;
    readonly webcanvas_panBy: (a: number, b: number, c: number) => number;
    readonly webcanvas_pushStrokeSamples: (a: number, b: number, c: number) => number;
    readonly webcanvas_redo: (a: number) => number;
    readonly webcanvas_render: (a: number) => number;
    readonly webcanvas_resize: (a: number, b: number, c: number, d: number) => void;
    readonly webcanvas_setBrushSize: (a: number, b: number) => void;
    readonly webcanvas_setColor: (a: number, b: number, c: number, d: number) => void;
    readonly webcanvas_setTool: (a: number, b: number) => [number, number];
    readonly webcanvas_undo: (a: number) => number;
    readonly webcanvas_zoomAt: (a: number, b: number, c: number, d: number) => number;
    readonly wasm_bindgen__convert__closures_____invoke__h09894e96c6d9a5b6: (a: number, b: number, c: any) => [number, number];
    readonly wasm_bindgen__convert__closures_____invoke__h443b382ee22a5c11: (a: number, b: number, c: any, d: any) => void;
    readonly wasm_bindgen__convert__closures_____invoke__h61d099fbc74d6f11: (a: number, b: number, c: any) => void;
    readonly __wbindgen_malloc: (a: number, b: number) => number;
    readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
    readonly __wbindgen_exn_store: (a: number) => void;
    readonly __externref_table_alloc: () => number;
    readonly __wbindgen_externrefs: WebAssembly.Table;
    readonly __wbindgen_destroy_closure: (a: number, b: number) => void;
    readonly __externref_table_dealloc: (a: number) => void;
    readonly __wbindgen_start: () => void;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;

/**
 * Instantiates the given `module`, which can either be bytes or
 * a precompiled `WebAssembly.Module`.
 *
 * @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
 *
 * @returns {InitOutput}
 */
export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

/**
 * If `module_or_path` is {RequestInfo} or {URL}, makes a request and
 * for everything else, calls `WebAssembly.instantiate` directly.
 *
 * @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
 *
 * @returns {Promise<InitOutput>}
 */
export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;
