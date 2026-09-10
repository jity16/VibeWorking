import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const invoke = vi.fn()
vi.mock('@tauri-apps/api/core', () => ({ invoke: (name: string, args?: Record<string, unknown>) => invoke(name, args) }))
vi.mock('@tauri-apps/api/event', () => ({ listen: () => Promise.resolve(() => {}) }))

const App = (await import('./App')).default

const emptySettings = { base_url: '', protocol: 'responses', model: '', timeout_seconds: 90, api_key_ref: null, has_api_key: false }
const project = { id: 'p1', name: '端砚', root_path: '/tmp', context: '', constraints: '', archived: false, sort_order: 0, created_at: '', updated_at: '' }

let container: HTMLDivElement
let root: Root

/** React owns `value`, so a plain assignment is discarded — go through the native setter. */
function type(input: HTMLInputElement, value: string) {
  Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, 'value')!.set!.call(input, value)
  input.dispatchEvent(new Event('input', { bubbles: true }))
}

const flush = () => act(async () => { await Promise.resolve() })

beforeEach(() => {
  ;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true
  container = document.createElement('div')
  document.body.appendChild(container)
  root = createRoot(container)
  invoke.mockReset()
})

afterEach(() => {
  act(() => root.unmount())
  container.remove()
})

describe('creating the first project', () => {
  it('shows the project after it is saved, instead of dropping the result', async () => {
    // The database starts empty, so the only create-project button in the main
    // pane is the one inside the empty state. It used to be wired to a no-op:
    // the row reached SQLite and the list never changed.
    let saved = false
    invoke.mockImplementation((name: string) => {
      if (name === 'bootstrap') return Promise.resolve({ projects: saved ? [project] : [], tasks: [], sessions: [], settings: emptySettings })
      if (name === 'create_project') { saved = true; return Promise.resolve(project) }
      return Promise.reject(new Error(`unexpected command ${name}`))
    })

    await act(async () => { root.render(<App />) })
    await flush()

    const emptyState = container.querySelector('.empty-state')
    expect(emptyState, 'an empty database should offer a way to create the first project').not.toBeNull()

    const open = emptyState!.querySelector<HTMLButtonElement>('.small-add')!
    await act(async () => { open.click() })

    const inputs = emptyState!.querySelectorAll<HTMLInputElement>('.inline-form input')
    expect(inputs).toHaveLength(2)
    await act(async () => { type(inputs[0], '端砚'); type(inputs[1], '/tmp') })
    await act(async () => { emptyState!.querySelector('form')!.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true })) })
    await flush()

    expect(invoke).toHaveBeenCalledWith('create_project', { input: { name: '端砚', root_path: '/tmp' } })
    expect(container.querySelector('.project-list')!.textContent).toContain('端砚')
  })
})

describe('a project with no tasks', () => {
  it('offers exactly one way to create a task', async () => {
    // The pane heading already carries the create action. The empty state used
    // to render a second, identically styled primary button next to it.
    invoke.mockImplementation((name: string) =>
      name === 'bootstrap'
        ? Promise.resolve({ projects: [project], tasks: [], sessions: [], settings: emptySettings })
        : Promise.reject(new Error(`unexpected command ${name}`)),
    )

    await act(async () => { root.render(<App />) })
    await flush()

    const create = [...container.querySelectorAll('.list-pane button')].filter(button => button.textContent?.includes('新建任务'))
    expect(create).toHaveLength(1)
    expect(container.querySelector('.pane-heading')!.contains(create[0]), 'the create action belongs in the heading, where it stays put').toBe(true)
    expect(container.querySelector('.empty-state')!.textContent).toContain('新建任务')
  })

  it('opens the new-task form in the list, not inside the heading', async () => {
    invoke.mockImplementation((name: string) =>
      name === 'bootstrap'
        ? Promise.resolve({ projects: [project], tasks: [], sessions: [], settings: emptySettings })
        : Promise.reject(new Error(`unexpected command ${name}`)),
    )

    await act(async () => { root.render(<App />) })
    await flush()
    const open = [...container.querySelectorAll<HTMLButtonElement>('.pane-heading button')].find(button => button.textContent?.includes('新建任务'))!
    await act(async () => { open.click() })

    const form = container.querySelector('.new-task-form')
    expect(form, 'the form should render once opened').not.toBeNull()
    expect(container.querySelector('.pane-heading')!.contains(form!), 'the form belongs where the new row will land').toBe(false)
    expect(container.querySelector('.list-pane')!.contains(form!)).toBe(true)
  })
})

describe('running agents', () => {
  const live = (over: Record<string, unknown>) => ({ provider: 'codex', tmux_session: 'work', pane: '%1', pid: 1, cwd: '/x', uptime: '01:00', attached: false, project_id: null, project_name: null, ...over })

  const withSessions = (sessions: Record<string, unknown>[]) => {
    invoke.mockImplementation((name: string) => {
      if (name === 'bootstrap') return Promise.resolve({ projects: [project], tasks: [], sessions: [], settings: emptySettings })
      if (name === 'live_sessions') return Promise.resolve({ warnings: [], sessions })
      return Promise.reject(new Error(`unexpected command ${name}`))
    })
  }

  const openAgents = async () => {
    await act(async () => { root.render(<App />) })
    await flush()
    await act(async () => { [...container.querySelectorAll<HTMLButtonElement>('.tab')].find(tab => tab.textContent?.includes('Agents'))!.click() })
    await flush()
  }

  it('groups by project across all projects, with 未分类 last', async () => {
    withSessions([
      live({ pane: '%1', project_id: 'p1', project_name: '端砚', cwd: '/tmp/a' }),
      live({ pane: '%2', cwd: '/elsewhere' }),
      live({ pane: '%3', provider: 'claude', cwd: '/elsewhere/two' }),
    ])
    await openAgents()

    const groups = [...container.querySelectorAll('.group-heading')].map(heading => heading.textContent)
    expect(groups[0]).toContain('端砚')
    expect(groups[1]).toContain('未分类')
    expect(container.querySelectorAll('.agent-row')).toHaveLength(3)
  })

  it('shows only the selected project\'s sessions', async () => {
    withSessions([
      live({ pane: '%1', project_id: 'p1', project_name: '端砚', cwd: '/tmp/a' }),
      live({ pane: '%2', cwd: '/elsewhere' }),
    ])
    await openAgents()
    // Selecting a project in the sidebar scopes the Agents view too.
    await act(async () => { [...container.querySelectorAll<HTMLButtonElement>('.project-row')].find(row => row.textContent?.includes('端砚'))!.click() })
    await flush()

    const rows = [...container.querySelectorAll('.agent-row')]
    expect(rows).toHaveLength(1)
    expect(rows[0].textContent).toContain('/tmp/a')
    expect(container.querySelectorAll('.group-heading'), 'a single project needs no grouping').toHaveLength(0)
  })

  it('says the project has nothing running rather than showing an empty list', async () => {
    withSessions([live({ pane: '%2', cwd: '/elsewhere' })])
    await openAgents()
    await act(async () => { [...container.querySelectorAll<HTMLButtonElement>('.project-row')].find(row => row.textContent?.includes('端砚'))!.click() })
    await flush()

    expect(container.querySelectorAll('.agent-row')).toHaveLength(0)
    expect(container.querySelector('.empty-state')!.textContent).toContain('端砚')
  })

  it('focuses the pane it actually lists', async () => {
    withSessions([live({ tmux_session: 'cat', pane: '%35', project_id: 'p1', project_name: '端砚' })])
    await openAgents()
    await act(async () => { container.querySelector<HTMLButtonElement>('.agent-actions button')!.click() })

    expect(invoke).toHaveBeenCalledWith('focus_tmux_session', { session: 'cat', pane: '%35' })
  })
})
