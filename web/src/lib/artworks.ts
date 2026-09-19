type StoredLayer = {
  id: number;
  name: string;
  visible: boolean;
  opacity: number;
  clipped: boolean;
  png: Uint8Array;
};

export type StoredDocument = {
  schemaVersion: number;
  width: number;
  height: number;
  background: [number, number, number];
  brushColor: [number, number, number, number];
  selectedLayer: number;
  layers: StoredLayer[];
};

export type ArtworkRecord = {
  schemaVersion: 1;
  id: string;
  title: string;
  createdAt: number;
  updatedAt: number;
  document: StoredDocument;
};

const DATABASE_NAME = "chromazen";
const DATABASE_VERSION = 1;
const STORE_NAME = "artworks";
let databasePromise: Promise<IDBDatabase> | undefined;

function requestResult<T>(request: IDBRequest<T>): Promise<T> {
  return new Promise((resolve, reject) => {
    request.onsuccess = () => resolve(request.result);
    request.onerror = () =>
      reject(request.error ?? new Error("IndexedDB request failed"));
  });
}

function transactionDone(transaction: IDBTransaction): Promise<void> {
  return new Promise((resolve, reject) => {
    transaction.oncomplete = () => resolve();
    transaction.onabort = () =>
      reject(transaction.error ?? new Error("IndexedDB transaction aborted"));
    transaction.onerror = () =>
      reject(transaction.error ?? new Error("IndexedDB transaction failed"));
  });
}

function openDatabase(): Promise<IDBDatabase> {
  const opening =
    databasePromise ??
    new Promise<IDBDatabase>((resolve, reject) => {
      const request = indexedDB.open(DATABASE_NAME, DATABASE_VERSION);
      request.onupgradeneeded = () => {
        const store = request.result.createObjectStore(STORE_NAME, {
          keyPath: "id",
        });
        store.createIndex("updatedAt", "updatedAt");
      };
      request.onsuccess = () => {
        const database = request.result;
        database.onversionchange = () => database.close();
        resolve(database);
      };
      request.onerror = () =>
        reject(request.error ?? new Error("Could not open IndexedDB"));
      request.onblocked = () =>
        reject(new Error("Close other Chromazen tabs to update local storage"));
    }).catch((error) => {
      databasePromise = undefined;
      throw error;
    });
  databasePromise = opening;
  return opening;
}

export async function getArtwork(
  id: string,
): Promise<ArtworkRecord | undefined> {
  const database = await openDatabase();
  const transaction = database.transaction(STORE_NAME);
  const done = transactionDone(transaction);
  const value = await requestResult(
    transaction.objectStore(STORE_NAME).get(id),
  );
  await done;
  if (value === undefined) return undefined;
  if (!isArtworkRecord(value))
    throw new Error("The saved artwork has an unsupported format");
  return value;
}

// ponytail: this loads layer bytes too; split metadata into its own store if galleries become slow.
export async function listArtworks(): Promise<ArtworkRecord[]> {
  const database = await openDatabase();
  const transaction = database.transaction(STORE_NAME);
  const done = transactionDone(transaction);
  const values = await requestResult(
    transaction.objectStore(STORE_NAME).index("updatedAt").getAll(),
  );
  await done;
  return values.filter(isArtworkRecord).reverse();
}

export async function putArtwork(artwork: ArtworkRecord): Promise<void> {
  const database = await openDatabase();
  const transaction = database.transaction(STORE_NAME, "readwrite");
  const done = transactionDone(transaction);
  await requestResult(transaction.objectStore(STORE_NAME).put(artwork));
  await done;
}

export async function deleteArtwork(id: string): Promise<void> {
  const database = await openDatabase();
  const transaction = database.transaction(STORE_NAME, "readwrite");
  const done = transactionDone(transaction);
  await requestResult(transaction.objectStore(STORE_NAME).delete(id));
  await done;
}

function isArtworkRecord(value: unknown): value is ArtworkRecord {
  if (
    !isObject(value) ||
    value.schemaVersion !== 1 ||
    typeof value.id !== "string"
  )
    return false;
  if (
    typeof value.title !== "string" ||
    !isFiniteNumber(value.createdAt) ||
    !isFiniteNumber(value.updatedAt)
  )
    return false;
  const document = value.document;
  if (!isObject(document) || document.schemaVersion !== 1) return false;
  if (!isPositiveInteger(document.width) || !isPositiveInteger(document.height))
    return false;
  if (
    !isByteTuple(document.background, 3) ||
    !isByteTuple(document.brushColor, 4)
  )
    return false;
  if (
    !isPositiveInteger(document.selectedLayer) ||
    !Array.isArray(document.layers) ||
    document.layers.length === 0
  )
    return false;
  return document.layers.every(
    (layer) =>
      isObject(layer) &&
      isPositiveInteger(layer.id) &&
      typeof layer.name === "string" &&
      layer.name.trim().length > 0 &&
      typeof layer.visible === "boolean" &&
      isOpacity(layer.opacity) &&
      typeof layer.clipped === "boolean" &&
      layer.png instanceof Uint8Array,
  );
}

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function isFiniteNumber(value: unknown): value is number {
  return typeof value === "number" && Number.isFinite(value);
}

function isPositiveInteger(value: unknown): value is number {
  return typeof value === "number" && Number.isInteger(value) && value > 0;
}

function isOpacity(value: unknown): value is number {
  return (
    typeof value === "number" &&
    Number.isInteger(value) &&
    value >= 0 &&
    value <= 100
  );
}

function isByteTuple(value: unknown, length: number): boolean {
  return (
    Array.isArray(value) &&
    value.length === length &&
    value.every((item) => Number.isInteger(item) && item >= 0 && item <= 255)
  );
}
