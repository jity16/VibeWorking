export const taskStatus:{key:'todo'|'in_progress'|'review'|'done'; label:string}[] = [
  {key:'todo',label:'待办'}, {key:'in_progress',label:'进行中'}, {key:'review',label:'待验收'}, {key:'done',label:'已完成'}
]
export const statusLabel = (value:string) => taskStatus.find(item => item.key === value)?.label ?? '未知'
export const executionLabel = (value:string) => ({starting:'启动中',running:'运行中',waiting_input:'等待处理',backoff:'等待恢复',completed:'已结束',failed:'失败',stopped:'已停止',unknown:'状态未知'} as Record<string,string>)[value] ?? value
export const attentionLabel = (value:string) => ({input_required:'需要输入',approval_required:'需要审批',recovery_failed:'恢复失败',unverified:'待核验'} as Record<string,string>)[value] ?? ''
