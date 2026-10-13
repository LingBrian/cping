// 与 Rust 侧唯一的通道。所有业务逻辑（目标生成、调度、分类、统计、导出）
// 都在 cping-core 中，前端只转发与展示。

import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import type { AppConfigDto, ProbeEventDto, SnapshotDto } from '../shared/types'

export function isTauri(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window
}

export async function parsePrefix(input: string): Promise<string> {
  return invoke<string>('parse_prefix_cmd', { input })
}

export async function startScan(
  prefix: string,
  timeoutMs: number,
  concurrency: number,
): Promise<string> {
  return invoke<string>('start_scan', { prefix, timeoutMs, concurrency })
}

export async function cancelScan(): Promise<void> {
  await invoke('cancel_scan')
}

export async function snapshot(): Promise<SnapshotDto> {
  return invoke<SnapshotDto>('snapshot')
}

export async function exportResults(path: string): Promise<string> {
  return invoke<string>('export_results', { path })
}

/**
 * 移动端（Android）保存路径：不走桌面文件对话框，
 * 由 Rust 侧写到应用可写的公共目录并回报完整路径。
 */
export async function exportToAppDir(format: 'csv' | 'txt'): Promise<string> {
  return invoke<string>('export_to_app_dir', { format })
}

export async function loadConfig(): Promise<AppConfigDto> {
  return invoke<AppConfigDto>('load_config')
}

export async function saveConfig(config: AppConfigDto): Promise<void> {
  await invoke('save_config', { config })
}

export async function localAddresses(): Promise<string[]> {
  return invoke<string[]>('get_local_addresses')
}

export function onProbeEvent(cb: (event: ProbeEventDto) => void): Promise<UnlistenFn> {
  return listen<ProbeEventDto>('probe-event', (ev) => cb(ev.payload))
}
