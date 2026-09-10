import { describe, expect, it } from 'vitest'
import { attentionLabel, executionLabel, statusLabel } from './labels'

describe('domain labels', () => {
  it('keeps task review distinct from completion', () => {
    expect(statusLabel('review')).toBe('待验收')
    expect(statusLabel('done')).toBe('已完成')
  })
  it('names unknown provider state instead of guessing success', () => {
    expect(executionLabel('unknown')).toBe('状态未知')
    expect(attentionLabel('approval_required')).toBe('需要审批')
  })
})
