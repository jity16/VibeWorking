import { useEffect, useMemo, useState } from 'react'
import { Bot, Check, ChevronDown, ChevronRight, CircleAlert, Copy, MoreHorizontal, Pause, Play, Plus, RefreshCw, Search, Settings, Square, Terminal, X } from 'lucide-react'
import { agentEvents, command } from './lib/bridge'
import { attentionLabel, executionLabel, statusLabel } from './lib/labels'
import type { AgentSession, Bootstrap, DiscoveredSession, DiscoveryReport, Project, ProxySettings, Task } from './types/domain'

type View = 'tasks'|'agents'|'settings'
const emptyBootstrap:Bootstrap = {projects:[],tasks:[],sessions:[],settings:{base_url:'',protocol:'responses',model:'',timeout_seconds:90,api_key_ref:null,has_api_key:false}}

function App() {
  const [data,setData] = useState(emptyBootstrap); const [view,setView] = useState<View>('tasks'); const [projectId,setProjectId] = useState('all'); const [selectedTask,setSelectedTask] = useState<string|null>(null); const [error,setError] = useState(''); const [query,setQuery] = useState('')
  const refresh = () => command<Bootstrap>('bootstrap').then(next => {setData(next); setError('')}).catch(e => setError(String(e)))
  useEffect(() => { refresh(); let stop:undefined|(()=>void); agentEvents(() => { refresh() }).then(unlisten => {stop=unlisten}).catch(() => {}); return () => stop?.() }, [])
  const visibleTasks = useMemo(() => data.tasks.filter(task => (projectId==='all'||task.project_id===projectId) && (!query||`${task.title} ${task.original_request}`.toLowerCase().includes(query.toLowerCase()))),[data.tasks,projectId,query])
  const activeTask = data.tasks.find(task => task.id===selectedTask) ?? visibleTasks[0] ?? null
  return <div className="app-shell">
    <aside className="sidebar">
      <div className="brand"><span className="brand-mark">V</span><span>Vibe Working</span></div>
      <div className="side-section-title"><span>项目</span><CreateProject onCreated={project => {setProjectId(project.id);refresh()}}/></div>
      <nav className="project-list">
        <button className={`project-row ${projectId==='all'?'selected':''}`} onClick={()=>setProjectId('all')}><span className="project-dot all-dot"/>全部项目<span className="count">{data.tasks.length}</span></button>
        {data.projects.map(project => <button key={project.id} className={`project-row ${projectId===project.id?'selected':''}`} onClick={()=>setProjectId(project.id)}><span className="project-dot"/>{project.name}<span className="count">{data.tasks.filter(t=>t.project_id===project.id).length}</span></button>)}
      </nav>
      <div className="sidebar-bottom"><button className={view==='settings'?'nav-button active':'nav-button'} onClick={()=>setView('settings')}><Settings size={16}/>设置</button><div className="storage-note"><span className="live-dot"/>本地数据库</div></div>
    </aside>
    <main className="main-area">
      <header className="topbar"><div className="view-tabs"><button className={view==='tasks'?'tab active':'tab'} onClick={()=>setView('tasks')}>TODO</button><button className={view==='agents'?'tab active':'tab'} onClick={()=>setView('agents')}>Agents{data.sessions.filter(s=>s.attention!=='none').length>0&&<span className="attention-count">{data.sessions.filter(s=>s.attention!=='none').length}</span>}</button></div><div className="top-actions"><label className="search"><Search size={15}/><input value={query} onChange={e=>setQuery(e.target.value)} placeholder="搜索任务"/></label><button className="icon-button" title="刷新" onClick={refresh}><RefreshCw size={16}/></button></div></header>
      {error&&<div className="error-bar"><CircleAlert size={16}/><span>{error}</span><button onClick={()=>setError('')}><X size={15}/></button></div>}
      {view==='tasks'&&<TaskView projects={data.projects} tasks={visibleTasks} activeTask={activeTask} selectedProject={projectId} query={query} onSelect={setSelectedTask} onSelectProject={setProjectId} onCreated={task=>{setSelectedTask(task.id);refresh()}} onChanged={refresh}/>}
      {view==='agents'&&<AgentView projects={data.projects} tasks={data.tasks} sessions={data.sessions} onChanged={refresh}/>} 
      {view==='settings'&&<SettingsView settings={data.settings} onSaved={settings=>setData(old=>({...old,settings}))} onRestored={refresh}/>}
    </main>
  </div>
}

function TaskView({projects,tasks,activeTask,selectedProject,query,onSelect,onSelectProject,onCreated,onChanged}:{projects:Project[];tasks:Task[];activeTask:Task|null;selectedProject:string;query:string;onSelect:(id:string)=>void;onSelectProject:(id:string)=>void;onCreated:(task:Task)=>void;onChanged:()=>void}) {
  const [creating,setCreating] = useState(false)
  const grouped = selectedProject==='all'; const groups = grouped ? projects.map(project=>({project,tasks:tasks.filter(task=>task.project_id===project.id)})).filter(group=>group.tasks.length) : [{project:projects.find(p=>p.id===selectedProject),tasks}]
  const targetProject = selectedProject==='all'?(projects[0]?.id??''):selectedProject
  // The form opens where the new row will land, not up in the heading beside
  // the button that opened it.
  const body = creating
    ? <NewTaskForm projects={projects} projectId={targetProject} onCreated={task=>{setCreating(false);onCreated(task)}} onCancel={()=>setCreating(false)}/>
    : !projects.length ? <EmptyState text="先创建一个项目" action={<CreateProject onCreated={project=>{onSelectProject(project.id);onChanged()}}/>}/>
    : !tasks.length ? <EmptyState text={query?`没有匹配「${query}」的任务`:'还没有任务，用右上角的「新建任务」开始'}/>
    : null
  return <div className="content-grid"><section className="list-pane"><div className="pane-heading"><div><p className="eyebrow">{selectedProject==='all'?'工作区':'项目'}</p><h1>{selectedProject==='all'?'全部任务':projects.find(p=>p.id===selectedProject)?.name??'任务'}</h1></div>{!!projects.length&&!creating&&<button className="button primary" onClick={()=>setCreating(true)}><Plus size={15}/>新建任务</button>}</div>{body}{!!tasks.length&&<div className="task-groups">{groups.map(group=>group.project&&<TaskGroup key={group.project.id} project={group.project} tasks={group.tasks} grouped={grouped} activeId={activeTask?.id} onSelect={onSelect} onChanged={onChanged}/>)}</div>}</section><TaskInspector task={activeTask} projects={projects} onChanged={onChanged}/></div>
}

function TaskGroup({project,tasks,grouped,activeId,onSelect,onChanged}:{project:Project;tasks:Task[];grouped:boolean;activeId?:string;onSelect:(id:string)=>void;onChanged:()=>void}) { const [open,setOpen]=useState(true); return <div className="task-group">{grouped&&<button className="group-heading" onClick={()=>setOpen(!open)}>{open?<ChevronDown size={15}/>:<ChevronRight size={15}/>}<span>{project.name}</span><span className="group-count">{tasks.length}</span></button>}{open&&tasks.map(task=><TaskRow key={task.id} task={task} active={task.id===activeId} onClick={()=>onSelect(task.id)} onChanged={onChanged}/>)}</div> }
function TaskRow({task,active,onClick,onChanged}:{task:Task;active:boolean;onClick:()=>void;onChanged:()=>void}) { return <div className={`task-row ${active?'active':''}`} onClick={onClick}><span className={`status-mark ${task.status}`}/><div className="task-copy"><strong>{task.title}</strong><span>{task.original_request}</span></div><span className={`task-state ${task.status}`}>{statusLabel(task.status)}</span><button className="row-menu" title="删除任务" onClick={event=>{event.stopPropagation();if(confirm('删除这个任务及其执行历史？'))command('delete_task',{id:task.id}).then(onChanged)}}><MoreHorizontal size={16}/></button></div> }

function TaskInspector({task,projects,onChanged}:{task:Task|null;projects:Project[];onChanged:()=>void}) {
  const [prompt,setPrompt]=useState(''); const [optimizing,setOptimizing]=useState(false); const [message,setMessage]=useState(''); const [provider,setProvider]=useState('codex')
  useEffect(()=>setPrompt(task?.current_prompt??task?.original_request??''),[task?.id,task?.current_prompt,task?.original_request])
  if(!task)return <aside className="inspector empty-inspector"><div className="empty-icon">○</div><p>选择一个任务查看详情</p></aside>
  const project=projects.find(item=>item.id===task.project_id)
  const update=(input:Record<string,unknown>)=>command<Task>('update_task',{input:{id:task.id,project_id:task.project_id,title:task.title,original_request:task.original_request,current_prompt:task.current_prompt,status:task.status,...input}}).then(onChanged).catch(e=>setMessage(String(e)))
  const optimize=()=>{setOptimizing(true);setMessage('');command<string>('optimize_prompt',{input:{task_id:task.id,include_context:true}}).then(result=>{setPrompt(result);return command('save_prompt_draft',{taskId:task.id,content:result})}).catch(e=>setMessage(String(e))).finally(()=>setOptimizing(false))}
  return <aside className="inspector"><div className="inspector-header"><div><p className="eyebrow">任务详情</p><h2>{task.title}</h2></div><span className={`pill ${task.status}`}>{statusLabel(task.status)}</span></div><div className="trail"><span className="trail-node done"><Check size={12}/></span><span className="trail-line"/><span className="trail-node current"><Bot size={13}/></span><div><strong>{task.latest_run_status?executionLabel(task.latest_run_status):'尚未执行'}</strong><small>{project?.name??'未知项目'}</small></div></div><label className="field-label">原始需求</label><textarea className="request-box" value={task.original_request} onChange={e=>update({original_request:e.target.value})}/><div className="prompt-heading"><label className="field-label">采用的 Prompt</label><button className="text-button" onClick={()=>navigator.clipboard?.writeText(prompt)}><Copy size={13}/>复制</button></div><textarea className="prompt-box" value={prompt} onChange={e=>{setPrompt(e.target.value);command('save_prompt_draft',{taskId:task.id,content:e.target.value}).then(()=>setMessage('')).catch(e=>setMessage(`草稿未保存：${e}`))}} placeholder="保存任务的执行 Prompt…"/><div className="inspector-actions"><button className="button secondary" disabled={optimizing} onClick={optimize}>{optimizing?'生成中…':'优化 Prompt'}</button><button className="button primary" onClick={()=>command('adopt_prompt',{taskId:task.id,content:prompt}).then(()=>{setMessage('Prompt 已采用');onChanged()}).catch(e=>setMessage(String(e)))}>采用</button></div><div className="run-controls"><select value={provider} onChange={e=>setProvider(e.target.value)}><option value="codex">Codex</option><option value="claude">Claude Code</option></select><button className="button run" onClick={()=>command('start_run',{input:{task_id:task.id,provider,auto_retry:false}}).then(onChanged).catch(e=>setMessage(String(e)))}><Play size={14}/>交给 {provider==='codex'?'Codex':'Claude Code'}</button></div><div className="status-message" aria-live="polite">{message}</div></aside>
}


function AgentView({projects,tasks,sessions,onChanged}:{projects:Project[];tasks:Task[];sessions:AgentSession[];onChanged:()=>void}) {
  return <section className="agents-page">
    <div className="pane-heading"><div><p className="eyebrow">执行记录</p><h1>Agents</h1></div></div>
    <h3 className="section-title">本应用发起的执行</h3>
    {!sessions.length
      ? <EmptyState text="还没有从这里发起过执行。只有这一节里的会话是本应用启动并持续跟踪的。"/>
      : <div className="agent-list">{sessions.map(session=><AgentRow key={session.id} session={session} task={tasks.find(t=>t.id===session.task_id)} project={projects.find(p=>p.id===session.project_id)} onChanged={onChanged}/>)}</div>}
    <DiscoveredSessions/>
  </section>
}

/** Scanning spawns a codex app-server and reads transcripts off disk, so it runs
 *  when this tab is opened rather than on every bootstrap. */
function DiscoveredSessions() {
  const [report,setReport] = useState<DiscoveryReport|null>(null); const [loading,setLoading] = useState(true); const [error,setError] = useState('')
  const scan = () => {setLoading(true);setError('');command<DiscoveryReport>('discovered_sessions').then(setReport).catch(e=>setError(String(e))).finally(()=>setLoading(false))}
  useEffect(scan,[])
  const groups = useMemo(() => {
    const byProject = new Map<string,{name:string;sessions:DiscoveredSession[]}>()
    for(const session of report?.sessions??[]) {
      const key = session.project_id ?? ''
      if(!byProject.has(key)) byProject.set(key,{name:session.project_name??'未分类',sessions:[]})
      byProject.get(key)!.sessions.push(session)
    }
    // Named projects first; 未分类 is a leftover pile, not a peer.
    return [...byProject.entries()].sort((left,right)=>(left[0]?0:1)-(right[0]?0:1)||right[1].sessions.length-left[1].sessions.length)
  },[report])
  return <div className="discovered">
    <div className="section-heading"><h3 className="section-title">机器上已有的会话</h3><button className="text-button" onClick={scan} disabled={loading}>{loading?'扫描中…':'重新扫描'}</button></div>
    <p className="muted">从 Codex app-server 和 ~/.claude/projects 读取，按工作目录归到项目下。只读——本应用没有启动它们，无法确认是否还在运行，也不能停止或接管。</p>
    {error&&<p className="form-error">{error}</p>}
    {report?.warnings.map(warning=><p key={warning} className="form-error">{warning}</p>)}
    {loading&&!report&&<p className="muted">正在扫描…</p>}
    {report&&!report.sessions.length&&!loading&&<p className="muted">没有找到会话。</p>}
    {/* 未分类 is collapsed only when there are project groups it would bury.
        When it is all there is, collapsing it would show the user nothing. */}
    {groups.map(([id,group])=><DiscoveredGroup key={id||'unassigned'} name={group.name} sessions={group.sessions} defaultOpen={!!id||groups.length===1}/>)}
    {report?.truncated&&<p className="muted">Codex 会话很多，这里只列出最近的部分。</p>}
  </div>
}

function DiscoveredGroup({name,sessions,defaultOpen}:{name:string;sessions:DiscoveredSession[];defaultOpen:boolean}) {
  const [open,setOpen] = useState(defaultOpen)
  return <div className="task-group">
    <button className="group-heading" onClick={()=>setOpen(!open)}>{open?<ChevronDown size={15}/>:<ChevronRight size={15}/>}<span>{name}</span><span className="group-count">{sessions.length}</span></button>
    {open&&<div className="agent-list">{sessions.map(session=><div key={`${session.provider}-${session.session_id}`} className="agent-row discovered-row">
      <div className={`agent-icon ${session.provider}`}><Bot size={17}/></div>
      <div className="agent-main">
        <div className="agent-title"><strong>{session.title||'（无标题）'}</strong><span className="execution">{session.provider==='codex'?'Codex':'Claude Code'}</span></div>
        <span className="agent-task mono">{session.cwd}</span>
        <span className="agent-activity">{session.updated_at?new Date(session.updated_at).toLocaleString():'时间未知'}</span>
      </div>
      <button className="text-button" title="复制会话 ID" onClick={()=>navigator.clipboard?.writeText(session.session_id)}><Copy size={13}/>ID</button>
    </div>)}</div>}
  </div>
}
function AgentRow({session,task,project,onChanged}:{session:AgentSession;task?:Task;project?:Project;onChanged:()=>void}) {
  const needsAttention = session.attention !== 'none'
  return <div className={`agent-row ${needsAttention?'needs-attention':''}`}>
    <div className={`agent-icon ${session.provider}`}><Bot size={17}/></div>
    <div className="agent-main"><div className="agent-title"><strong>{session.display_name}</strong><span className={`execution ${session.execution_status}`}>{executionLabel(session.execution_status)}</span></div><span className="agent-task">{task?.title??'未关联任务'} · {project?.name??'未知项目'}</span><span className="agent-activity">{session.recent_activity??'等待活动'}</span></div>
    {needsAttention && <span className="attention-label"><CircleAlert size={14}/>{attentionLabel(session.attention)}</span>}
    <div className="agent-actions"><button title="定位终端" onClick={()=>command('focus_session',{sessionId:session.id}).catch(e=>alert(e))}><Terminal size={15}/></button>{session.execution_status!=='stopped'&&session.execution_status!=='completed'&&<button title="停止" onClick={()=>command('stop_session',{sessionId:session.id}).then(onChanged).catch(e=>alert(e))}><Square size={14}/></button>}<button title="人工接管" onClick={()=>command('take_over_session',{sessionId:session.id}).then(onChanged).catch(e=>alert(e))}><Pause size={14}/></button></div>
  </div>
}

function SettingsView({settings,onSaved,onRestored}:{settings:ProxySettings;onSaved:(settings:ProxySettings)=>void;onRestored:()=>void}) { const [form,setForm]=useState({...settings,api_key:''}); const [notice,setNotice]=useState(''); const save=()=>command<ProxySettings>('save_proxy_settings',{input:{base_url:form.base_url,protocol:form.protocol,model:form.model,timeout_seconds:Number(form.timeout_seconds),api_key:form.api_key||null}}).then(result=>{onSaved(result);setForm(old=>({...old,api_key:''}));setNotice('设置已保存')}).catch(e=>setNotice(String(e))); const test=()=>command<string>('test_proxy_connection').then(setNotice).catch(e=>setNotice(String(e))); return <section className="settings-page"><div className="pane-heading"><div><p className="eyebrow">应用</p><h1>设置</h1></div></div><div className="settings-card"><h3>Prompt 优化代理</h3><p className="muted">密钥只存入 macOS Keychain，不进入数据库导出。</p><label>Base URL<input value={form.base_url} onChange={e=>setForm({...form,base_url:e.target.value})} placeholder="https://proxy.example.com"/></label><label>协议<select value={form.protocol} onChange={e=>setForm({...form,protocol:e.target.value})}><option value="responses">Responses API</option><option value="chat_completions">Chat Completions API</option></select></label><label>模型<input value={form.model} onChange={e=>setForm({...form,model:e.target.value})} placeholder="手动输入模型名"/></label><label>API Key<input type="password" value={form.api_key} onChange={e=>setForm({...form,api_key:e.target.value})} placeholder={settings.has_api_key?'已保存，留空保持不变':'输入密钥'}/></label><div className="settings-actions"><button className="button secondary" onClick={test}>测试连接</button><button className="button primary" onClick={save}>保存设置</button></div><div className="status-message">{notice}</div></div><div className="settings-card"><h3>数据</h3><p className="muted">数据保存在 ~/Library/Application Support/Vibe Working/。</p><button className="button secondary" onClick={()=>command<string>('export_data').then(data=>{const blob=new Blob([data],{type:'application/json'});const link=document.createElement('a');link.href=URL.createObjectURL(blob);link.download='vibe-working-backup.json';link.click()}).catch(e=>setNotice(String(e)))}>导出备份</button><label className="button secondary file-button">导入备份<input type="file" accept="application/json,.json" onChange={event=>{const file=event.target.files?.[0];event.target.value='';if(!file)return;if(!confirm('导入只能写入空的工作库。继续？'))return;file.text().then(content=>command('restore_data',{content})).then(()=>{setNotice('备份已导入');onRestored()}).catch(e=>setNotice(String(e)))}}/></label><p className="muted">导入要求当前工作库没有项目；备份不含 API Key。</p></div></section> }

function EmptyState({text,action}:{text:string;action?:React.ReactNode}) { return <div className="empty-state"><p>{text}</p>{action}</div> }
function CreateProject({onCreated}:{onCreated:(project:Project)=>void}) { const [open,setOpen]=useState(false);const [name,setName]=useState('');const [path,setPath]=useState('');if(!open)return <button className="small-add" title="新建项目" onClick={()=>setOpen(true)}><Plus size={15}/></button>;return <form className="inline-form" onSubmit={e=>{e.preventDefault();command<Project>('create_project',{input:{name,root_path:path}}).then(project=>{onCreated(project);setOpen(false);setName('');setPath('')}).catch(error=>alert(error))}}><input autoFocus value={name} onChange={e=>setName(e.target.value)} placeholder="项目名称"/><input value={path} onChange={e=>setPath(e.target.value)} placeholder="本地路径"/><button className="small-add" type="submit"><Check size={15}/></button><button className="small-add" type="button" onClick={()=>setOpen(false)}><X size={15}/></button></form> }
function NewTaskForm({projects,projectId,onCreated,onCancel}:{projects:Project[];projectId:string;onCreated:(task:Task)=>void;onCancel:()=>void}) {
  const [title,setTitle]=useState(''); const [request,setRequest]=useState(''); const [error,setError]=useState('')
  const project=projects.find(item=>item.id===projectId)??projects[0]
  return <form className="new-task-form" onKeyDown={event=>{if(event.key==='Escape')onCancel()}} onSubmit={event=>{event.preventDefault();setError('');command<Task>('create_task',{input:{project_id:project.id,title,original_request:request}}).then(onCreated).catch(e=>setError(String(e)))}}>
    <input autoFocus value={title} onChange={e=>setTitle(e.target.value)} placeholder="任务标题"/>
    <textarea value={request} onChange={e=>setRequest(e.target.value)} placeholder="描述要完成的事情"/>
    <div className="new-task-footer"><span className="new-task-target">建到「{project.name}」</span><button className="button secondary" type="button" onClick={onCancel}>取消</button><button className="button primary" type="submit">创建任务</button></div>
    {error&&<p className="form-error">{error}</p>}
  </form>
}
export default App
