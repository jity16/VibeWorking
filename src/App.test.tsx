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

describe('discovered sessions', () => {
  const discovered = (over: Record<string, unknown>) => ({ provider: 'codex', session_id: 's', cwd: '/x', title: 't', preview: 'p', updated_at: '2026-09-10T00:00:00Z', project_id: null, project_name: null, ...over })

  it('groups by project, puts 未分类 last, and leaves it collapsed', async () => {
    // 未分类 is where everything the user never registered lands — hundreds of
    // rows on a real machine. Expanding it by default would bury the sessions
    // that belong to a project.
    invoke.mockImplementation((name: string) => {
      if (name === 'bootstrap') return Promise.resolve({ projects: [project], tasks: [], sessions: [], settings: emptySettings })
      if (name === 'discovered_sessions') return Promise.resolve({
        warnings: [],
        truncated: false,
        sessions: [
          discovered({ session_id: 'a', project_id: 'p1', project_name: '端砚', cwd: '/tmp/a' }),
          discovered({ session_id: 'b', cwd: '/elsewhere' }),
          discovered({ session_id: 'c', provider: 'claude', cwd: '/elsewhere/two' }),
        ],
      })
      return Promise.reject(new Error(`unexpected command ${name}`))
    })

    await act(async () => { root.render(<App />) })
    await flush()
    const agents = [...container.querySelectorAll<HTMLButtonElement>('.tab')].find(tab => tab.textContent?.includes('Agents'))!
    await act(async () => { agents.click() })
    await flush()

    const groups = [...container.querySelectorAll('.discovered .group-heading')].map(heading => heading.textContent)
    expect(groups).toHaveLength(2)
    expect(groups[0]).toContain('端砚')
    expect(groups[1]).toContain('未分类')
    expect(groups[1]).toContain('2')

    const rendered = [...container.querySelectorAll('.discovered .discovered-row')]
    expect(rendered, 'only the project group starts expanded').toHaveLength(1)

    await act(async () => { (container.querySelectorAll<HTMLButtonElement>('.discovered .group-heading')[1]).click() })
    expect(container.querySelectorAll('.discovered .discovered-row')).toHaveLength(3)
  })

  it('reports a provider failure without hiding the other provider', async () => {
    invoke.mockImplementation((name: string) => {
      if (name === 'bootstrap') return Promise.resolve({ projects: [project], tasks: [], sessions: [], settings: emptySettings })
      if (name === 'discovered_sessions') return Promise.resolve({
        warnings: ['Codex 会话未能读取：boom'],
        truncated: false,
        sessions: [discovered({ provider: 'claude', session_id: 'c' })],
      })
      return Promise.reject(new Error(`unexpected command ${name}`))
    })

    await act(async () => { root.render(<App />) })
    await flush()
    await act(async () => { [...container.querySelectorAll<HTMLButtonElement>('.tab')].find(tab => tab.textContent?.includes('Agents'))!.click() })
    await flush()

    expect(container.querySelector('.discovered .form-error')!.textContent).toContain('Codex 会话未能读取')
    expect(container.querySelectorAll('.discovered .discovered-row').length).toBeGreaterThan(0)
  })
})
