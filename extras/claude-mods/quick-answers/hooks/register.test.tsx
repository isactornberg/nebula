import type { PromptBox, PromptEditInput } from 'claude-code'
import { expect, test } from 'claude-code/testing'

import { GUIDE, answersOf, withoutNextLine } from './register'

const BAND = {
  plugin: 'quick-answers',
  component: 'AbovePrompt',
  props: {
    hasSurvey: false,
    isCollapsed: false,
    isWorking: false,
    maxRows: 4,
    bodyColumns: 100,
    scroll: { offset: 0, bodyRows: 4 },
    view: {},
  },
  surface: 'terminal',
} as const

const TURN = { durationMs: 1000, isAborted: false, turnId: 't1' } as const

test('a reply ending on a NEXT line offers its answers above the prompt', async ($, on) => {
  on('turn.complete', (_, e) => ({ text: e.answer }))
  on('ui.render', ($, e) => {
    const { Text } = $.ui.resolve(e)

    return <Text>engine</Text>
  })

  await $.turn.complete({
    ...TURN,
    reason: 'answer',
    answer: 'Done with the middleware.\n\nNEXT: Per token | Per IP | Per token, admins exempt',
  })

  const ui = await $.ui.mount(BAND)
  const first = await ui.find({ type: 'Button', key: 'pick-0' })
  const third = await ui.find({ type: 'Button', key: 'pick-2' })

  expect(first?.props.label).toBe('Per token')
  expect(third?.props.label).toBe('Per token, admins exempt')
  // Each answer is a pill: its digit, then its words. The first, the one the
  // reply recommends, is the filled one.
  expect((await ui.find({ type: 'Text', text: /admins exempt/ }))?.text).toBe(' Per token, admins exempt')
  expect((await ui.find({ type: 'Text', text: / Per token$/ }))?.props.bold).toBe(true)
  expect((await ui.find({ type: 'Text', text: / Per IP$/ }))?.props.bold).toBe(false)
  await ui.unmount()
})

test('a reply with no NEXT line leaves the band to the engine', async ($, on) => {
  on('turn.complete', (_, e) => ({ text: e.answer }))
  on('ui.render', ($, e) => {
    const { Text } = $.ui.resolve(e)

    return <Text>engine</Text>
  })

  await $.turn.complete({ ...TURN, reason: 'answer', answer: 'All tests pass.' })

  const ui = await $.ui.mount(BAND)

  expect((await ui.find({ type: 'Text', text: /engine/ }))?.text).toBe('engine')
  await ui.unmount()
})

test('a bare digit in the empty prompt box sends that answer', async ($, on) => {
  const sent: string[] = []
  on('turn.complete', (_, e) => ({ text: e.answer }))
  on('ui.render', ($, e) => {
    const { Text } = $.ui.resolve(e)

    return <Text>engine</Text>
  })
  on('prompt.edit', (_, e) => ({
    text: e.text.slice(0, e.start) + e.inputText + e.text.slice(e.end),
    cursor: e.start + e.inputText.length,
  }))
  on('prompt.submit', (_, e) => {
    sent.push(e.text)

    return { text: e.text }
  })

  await $.turn.complete({
    ...TURN,
    reason: 'answer',
    answer: 'Which one?\nNEXT: Per token | Per IP',
  })

  const edit = { origin: { kind: 'composer' }, cursor: 0, start: 0, end: 0 } as const

  // No row on screen yet, so a digit is text: nothing is picked blind.
  expect(
    (
      await ($ as unknown as { prompt: { edit: (e: PromptEditInput) => Promise<PromptBox> } }).prompt.edit({
        ...edit,
        text: '',
        inputText: '2',
      })
    ).text,
  ).toBe('2')

  const ui = await $.ui.mount(BAND)
  // The person's keystroke, raised as the composer raises it.
  const type = (e: PromptEditInput): Promise<PromptBox> =>
    ($ as unknown as { prompt: { edit: (e: PromptEditInput) => Promise<PromptBox> } }).prompt.edit(e)

  // A digit with no answer behind it, and one typed after other text, are text.
  expect((await type({ ...edit, text: '', inputText: '3' })).text).toBe('3')
  expect(
    (await type({ ...edit, text: 'v', cursor: 1, start: 1, end: 1, inputText: '2' })).text,
  ).toBe('v2')
  expect(sent).toEqual([])

  expect((await type({ ...edit, text: '', inputText: '2' })).text).toBe('')
  expect(sent).toEqual(['Per IP'])

  // Picked once: the same digit is text again.
  expect((await type({ ...edit, text: '', inputText: '2' })).text).toBe('2')
})

test('a reply draws without the NEXT line it ends on', () => {
  expect(withoutNextLine('Which one?\n\nNEXT: Per token | Per IP')).toBe('Which one?')
  expect(withoutNextLine('Which one?\n**NEXT:** Per token | Per IP\n')).toBe('Which one?')
  // A line further up, a reply with none, and one that is the line alone stay.
  expect(withoutNextLine('NEXT: a | b\nmore\nand more')).toBe('NEXT: a | b\nmore\nand more')
  expect(withoutNextLine('All tests pass.')).toBe('All tests pass.')
  expect(withoutNextLine('NEXT: a | b')).toBe('NEXT: a | b')
})

test('a session that draws is told to end a reply on a NEXT line, a headless one is not', async ($, on) => {
  const INTRO = { id: 'intro', text: 'You are Claude.', scope: 'shared' } as const
  on('prompt.compose', () => ({ sections: [INTRO] }))

  const compose = { model: 'm', promptModel: 'm', tools: [], outputStyle: null, traits: [] } as const
  const drawn = await $.prompt.compose({ ...compose, surfaces: ['terminal'] })
  const headless = await $.prompt.compose({ ...compose, surfaces: [] })

  expect(drawn.sections.map(section => section.id)).toEqual(['intro', GUIDE.id])
  expect(headless.sections.map(section => section.id)).toEqual(['intro'])
})

test('the NEXT line counts last or just before a closing line, and bars in code stay put', () => {
  expect(answersOf('Done.\nNEXT: Yes | No')).toEqual(['Yes', 'No'])
  expect(answersOf('Which port?\nNEXT: 8080 | 3000\nQUESTION which port should it use?')).toEqual(['8080', '3000'])
  expect(answersOf('NEXT: a | b\nmore\nand more')).toEqual([])
  expect(answersOf('Pick one.\nNEXT: Use `a || b` | Use `a ?? b`')).toEqual(['Use `a || b`', 'Use `a ?? b`'])
  expect(answersOf('- NEXT: one | two | three | four | five')).toEqual(['one', 'two', 'three', 'four'])

  expect(withoutNextLine('Which port?\nNEXT: 8080 | 3000\nQUESTION which port should it use?')).toBe(
    'Which port?\nQUESTION which port should it use?',
  )
  // A line that offers nothing draws no pills, so it is not hidden either.
  expect(withoutNextLine('Hm.\nNEXT: | |')).toBe('Hm.\nNEXT: | |')
})

test('answers too long for the band stack one to a row, each cut to fit', async ($, on) => {
  on('turn.complete', (_, e) => ({ text: e.answer }))
  on('ui.render', ($, e) => {
    const { Text } = $.ui.resolve(e)

    return <Text>engine</Text>
  })

  await $.turn.complete({
    ...TURN,
    reason: 'answer',
    answer: 'Which store?\nNEXT: SQLite, one file with zero setup | PostgreSQL, full SQL, run locally or hosted',
  })

  const ui = await $.ui.mount({ ...BAND, props: { ...BAND.props, bodyColumns: 30 } })
  const cut = await ui.find({ type: 'Text', text: /PostgreSQL/ })

  expect(cut?.text.endsWith('…')).toBe(true)
  expect((cut?.text.length ?? 99) + 4 <= 30).toBe(true)
  // The pick still sends the whole answer.
  expect((await ui.find({ type: 'Button', key: 'pick-1' }))?.props.label).toBe(
    'PostgreSQL, full SQL, run locally or hosted',
  )
  await ui.unmount()
})
