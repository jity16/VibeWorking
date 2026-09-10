import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'

export const command = <T>(name:string, args?:Record<string,unknown>) => invoke<T>(name, args)
export const agentEvents = (callback:(payload:unknown)=>void) => listen('agent-event', event => callback(event.payload))
