import { atom, read, update } from 'claude-code'
import type { Register } from 'claude-code'

import type { Answers } from '../types'

const MAX_ANSWERS = 4
const NONE: Answers = []

// Catppuccin Macchiato. The first answer is the one the reply recommends:
// its pill is filled mauve, the rest sit quiet on surface0.
const BASE = '#24273a'
const SURFACE = '#363a4f'
const SUBTEXT = '#a5adcb'
const MAUVE = '#c6a0f6'

// A pill's rounded ends: the powerline half circles of a Nerd Font, which a
// terminal draws in the pill's color on either side of its filled text.
const CAP_LEFT = '\ue0b6'
const CAP_RIGHT = '\ue0b4'

const answers = atom({ plugin: 'quick-answers', key: 'answers' } as const, NONE)

// What a session is told, so a reply that leaves something to decide ends on
// the line the pills are read from. Only where a surface draws the pills.
export const GUIDE = {
  id: 'quick-answers:next-line',
  scope: 'session',
  text:
    'Whenever your reply leaves the user with something to answer or decide (a question you asked, ' +
    'options you laid out, or finished work with a follow-up they may want), add one line `NEXT: <a> ' +
    '| <b> | <c>` as the very last line of the reply: 2 to 4 answers, each a few words the user could ' +
    'send as their next message (`Yes, go ahead`, `Option 2`, `Open a PR for it`), never a question, ' +
    'the one you recommend first. The user sees the answers as buttons in place of the line, and a ' +
    'pick is sent to you as it is. The line is an addition and replaces nothing: when other ' +
    'instructions have you end a reply on a particular line, such as a status or report line, write ' +
    'that line as you would and put the `NEXT:` line right after it. Leave the `NEXT:` line out when ' +
    'the reply leaves nothing to answer or follow up, and when you are a subagent reporting to the ' +
    'agent that started you.',
} as const

const NEXT_LINE = /^[\s*_`>-]*NEXT:[\s*_`]*(.+)$/

/**
 * Where a reply's `NEXT:` line is among its `lines`: its last line with
 * anything on it, or the one before that, since a session told to close on
 * a line of its own (a report line) may write that one last. -1 for none.
 */
function nextLineAt(lines: readonly string[]): number {
  const written = lines.flatMap((line, i) => (line.trim() === '' ? [] : [i]))

  return written.slice(-2).reverse().find(i => NEXT_LINE.test(lines[i] ?? '')) ?? -1
}

/**
 * A `NEXT:` line's answers: split on the bars that stand outside a code
 * span, so `a || b` in backticks stays one answer, at most four.
 */
function answersIn(line: string): Answers {
  const found: string[] = []
  let answer = ''
  let inCode = false

  for (const char of NEXT_LINE.exec(line)?.[1] ?? '') {
    if (char === '`') {
      inCode = !inCode
    }

    if (char === '|' && !inCode) {
      found.push(answer)
      answer = ''
    } else {
      answer += char
    }
  }

  found.push(answer)

  return found
    .map(text => text.replace(/^[\s*_]+|[\s*_]+$/g, ''))
    .filter(text => text !== '')
    .slice(0, MAX_ANSWERS)
}

/**
 * The answers a reply offers on its `NEXT: a | b | c` line, none when it
 * has no such line where one counts ([`nextLineAt`]).
 */
export function answersOf(reply: string): Answers {
  const lines = reply.split('\n')
  const at = nextLineAt(lines)

  return at < 0 ? NONE : answersIn(lines[at] ?? '')
}

/**
 * A reply's text as the transcript draws it: without the `NEXT:` line the
 * pills over the prompt stand in for. A line that offers no answer, and a
 * text that is that line and nothing else, are left as they are, so nothing
 * is hidden without pills and no reply draws empty.
 */
export function withoutNextLine(text: string): string {
  const lines = text.split('\n')
  const at = nextLineAt(lines)

  if (at < 0 || answersIn(lines[at] ?? '').length === 0) {
    return text
  }

  const rest = [...lines.slice(0, at), ...lines.slice(at + 1)].join('\n').trimEnd()

  return rest === '' ? text : rest
}

/** How many cells a pill takes: its caps, its digit, a space and its answer. */
function pillWidth(answer: string): number {
  return answer.length + 4
}

// Whether the row is on screen now: drawn, not collapsed, no survey in its
// place. A digit picks only an answer the person can see.
let isShown = false

export const register: Register = on => {
  on('prompt.compose', async ($, e, next) => {
    const composed = await next(e)
    const draws = e.surfaces.some(surface => surface === 'terminal' || surface === 'desktop')

    if (!draws) {
      return composed
    }

    return { sections: [...composed.sections.filter(section => section.id !== GUIDE.id), GUIDE] }
  })

  // A new turn, from a typed prompt or a pick, answers the last reply: its
  // row goes.
  on('turn.start', async ($, e, next) => {
    if (e.agentId === undefined) {
      await update($, answers, () => NONE)
    }

    return next(e)
  })

  on('turn.complete', async ($, e, next) => {
    // A subagent's turn ends inside the main one and offers the person nothing.
    if (e.agentId === undefined) {
      const offered = e.reason === 'answer' ? answersOf(e.answer) : NONE
      await update($, answers, () => offered)
    }

    return next(e)
  })

  // A bare digit typed into the empty prompt box picks that answer, as a
  // survey's row takes one: the key is consumed and the answer sent. A digit
  // typed anywhere else is text, and so is one typed while no row shows. The
  // answers are read from the state each time, the same place the row is
  // drawn from, so a cleared conversation leaves nothing behind to send.
  on('prompt.edit', async ($, e, next) => {
    const digit = e.text === '' && /^[1-4]$/.test(e.inputText) ? Number(e.inputText) : 0

    if (digit === 0 || !isShown) {
      return next(e)
    }

    const answer = (await read($, answers))[digit - 1]

    if (answer === undefined) {
      return next(e)
    }

    await update($, answers, () => NONE)
    void $.prompt.submit({ text: answer, asUser: true })

    return { text: e.text, cursor: e.cursor }
  }).catch(($, e, next) => next(e))

  // The line the pills are read from is theirs to show: the reply draws
  // without it. The stored message keeps it (ctrl+o shows it whole).
  on('ui.render', { component: 'AssistantMessage' }, ($, e, next) => {
    const text = withoutNextLine(e.props.text)

    return text === e.props.text ? next(e) : next({ ...e, props: { ...e.props, text } })
  })

  on('ui.render', { component: 'AbovePrompt' }, async ($, e, next) => {
    const offered = await read($, answers)
    const draws = !e.props.hasSurvey && offered.length > 0

    isShown = draws && !e.props.isCollapsed

    if (!draws) {
      return next(e)
    }

    const { Box, Button, Text } = $.ui.resolve(e)

    // Side by side while they fit the band; else one under another, each cut
    // to the band's width, so a pill never wraps into the next one's words.
    const room = e.props.bodyColumns
    const inRow = offered.reduce((cells, answer) => cells + 2 + pillWidth(answer), 0) <= room
    const longest = Math.max(1, room - 2 - pillWidth(''))

    return (
      <Box flexDirection={inRow ? 'row' : 'column'}>
        {offered.map((answer, i) => {
          const fill = i === 0 ? MAUVE : SURFACE
          const ink = i === 0 ? BASE : SUBTEXT
          const words =
            inRow || answer.length <= longest ? answer : `${answer.slice(0, longest - 1)}…`

          return (
            <Box key={`answer-${i}`}>
              <Text>{'  '}</Text>
              <Button
                key={`pick-${i}`}
                label={answer}
                plain
                onPress={() => {
                  void update($, answers, () => NONE)
                  void $.prompt.submit({ text: answer, asUser: true })
                }}
              >
                <Text color={fill}>{CAP_LEFT}</Text>
                <Text backgroundColor={fill} color={ink} bold>
                  {`${i + 1}`}
                </Text>
                <Text backgroundColor={fill} color={ink} bold={i === 0}>
                  {` ${words}`}
                </Text>
                <Text color={fill}>{CAP_RIGHT}</Text>
              </Button>
            </Box>
          )
        })}
      </Box>
    )
  })
}
