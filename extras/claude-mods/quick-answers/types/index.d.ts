export type Answers = readonly string[]

declare module 'claude-code' {
  interface PluginState {
    'quick-answers': { answers: Answers }
  }
}
