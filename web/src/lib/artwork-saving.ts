import { putArtwork, type StoredDocument } from './artworks'

export type SaveState = 'idle' | 'saving' | 'saved' | 'failed'

type SaveRenderer = {
  saveDocument(): Promise<StoredDocument>
}

type ArtworkSaverOptions = {
  getId: () => string
  getRenderer: () => SaveRenderer | undefined
  getCreatedAt: () => number
  isInteractionActive: () => boolean
  isMounted: () => boolean
  onStateChange: (state: SaveState) => void
  onError: (message: string) => void
  onManualSavedChange: (saved: boolean) => void
}

export class ArtworkSaver {
  private changeVersion = 0
  private savedVersion = 0
  private saveTimer: ReturnType<typeof setTimeout> | undefined
  private manualSavedTimer: ReturnType<typeof setTimeout> | undefined
  private saveInFlight: Promise<boolean> | undefined
  private persistenceReady = false
  private documentStored = false
  private persistRequested = false
  private allowUnload = false
  private disposed = false
  private readonly interactionWaiters: Array<() => void> = []

  constructor(private readonly options: ArtworkSaverOptions) {}

  setReady(documentStored: boolean) {
    this.persistenceReady = true
    this.documentStored = documentStored
  }

  saveInitial() {
    return this.save({ force: true })
  }

  markDirty() {
    this.changeVersion += 1
    this.options.onManualSavedChange(false)
    this.options.onStateChange(this.persistenceReady ? 'idle' : 'failed')
    if (this.persistenceReady) this.scheduleSave()
  }

  interactionEnded() {
    for (const resolve of this.interactionWaiters.splice(0)) resolve()
  }

  manualSave = async () => {
    if (!(await this.save({ flush: true }))) return
    this.options.onManualSavedChange(true)
    if (this.manualSavedTimer) clearTimeout(this.manualSavedTimer)
    this.manualSavedTimer = setTimeout(() => this.options.onManualSavedChange(false), 1800)
  }

  returnToGallery = async () => {
    const saved = await this.save({ flush: true })
    if (!saved && !window.confirm('Saving failed. Leave without saving?')) return
    this.allowUnload = true
    window.location.assign('/gallery')
  }

  beforeUnload = (event: BeforeUnloadEvent) => {
    if (
      this.allowUnload ||
      (!this.options.isInteractionActive() &&
        !this.saveInFlight &&
        this.documentStored &&
        this.savedVersion === this.changeVersion)
    )
      return
    event.preventDefault()
    event.returnValue = ''
  }

  dispose() {
    this.disposed = true
    if (this.saveTimer) clearTimeout(this.saveTimer)
    if (this.manualSavedTimer) clearTimeout(this.manualSavedTimer)
    this.interactionEnded()
  }

  private scheduleSave(delay = 2000) {
    if (this.saveTimer) clearTimeout(this.saveTimer)
    this.saveTimer = setTimeout(() => void this.save(), delay)
  }

  private waitForInteractionEnd() {
    if (!this.options.isInteractionActive()) return Promise.resolve()
    return new Promise<void>((resolve) => this.interactionWaiters.push(resolve))
  }

  private async performSave(version: number) {
    const renderer = this.options.getRenderer()
    if (!renderer) return false
    this.options.onStateChange('saving')
    try {
      const document = await renderer.saveDocument()
      if (this.disposed || !this.options.isMounted()) return false
      await putArtwork({
        schemaVersion: 1,
        id: this.options.getId(),
        title: 'Untitled',
        createdAt: this.options.getCreatedAt(),
        updatedAt: Date.now(),
        document,
      })
      this.savedVersion = version
      this.documentStored = true
      this.options.onError('')
      this.options.onStateChange(this.changeVersion === version ? 'saved' : 'idle')
      if (!this.persistRequested) {
        this.persistRequested = true
        void navigator.storage?.persist?.()
      }
      return true
    } catch (cause) {
      if (!this.disposed && this.options.isMounted()) {
        this.options.onError(cause instanceof Error ? cause.message : String(cause))
        this.options.onStateChange('failed')
      }
      return false
    }
  }

  private async save({ flush = false, force = false } = {}): Promise<boolean> {
    if (this.disposed || !this.options.getRenderer() || !this.persistenceReady) return false
    if (flush) {
      if (this.saveTimer) clearTimeout(this.saveTimer)
      this.saveTimer = undefined
      await this.waitForInteractionEnd()
      if (this.disposed || !this.options.isMounted()) return false
    } else if (this.options.isInteractionActive()) {
      // Serializing finishes the renderer's current stroke. An autosave queued by the
      // previous stroke must wait until the current pointer interaction has ended.
      this.scheduleSave(250)
      return false
    }

    while (this.saveInFlight) {
      if (!(await this.saveInFlight)) return false
      force = false
      if (!flush) return true
    }
    if (!force && this.documentStored && this.savedVersion === this.changeVersion) return true

    if (this.saveTimer) clearTimeout(this.saveTimer)
    this.saveTimer = undefined
    const version = this.changeVersion
    const pendingSave = this.performSave(version)
    this.saveInFlight = pendingSave
    const succeeded = await pendingSave
    if (this.saveInFlight === pendingSave) this.saveInFlight = undefined

    if (succeeded && !this.disposed && this.options.isMounted() && this.changeVersion > version) {
      if (flush) return this.save({ flush: true })
      this.scheduleSave()
    }
    return succeeded && (!flush || this.savedVersion === this.changeVersion)
  }
}
