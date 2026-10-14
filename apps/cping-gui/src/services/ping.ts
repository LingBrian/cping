// 与 Rust 侧唯一的通道。所有业务逻辑（目标生成、调度、分类、统计、导出）
// 都在 cping-core 中，前端只转发与展示。

import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import type {
  AppConfigDto,
  EnrichReportDto,
  HostnameLookupDto,
  PendingSnapshotDto,
  ProbeEventDto,
  SnapshotDto,
  SnapshotSavedDto,
} from '../shared/types'

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

// ---------------------------------------------------------------------------
// M7：目标信息补全
// ---------------------------------------------------------------------------

/**
 * 读一次本机邻居表（ARP）回填 MAC，并据 IEEE 注册数据补厂商。
 * 可重复调用：只补空缺，不会覆盖已有值。
 */
export async function enrichResults(): Promise<EnrichReportDto> {
  return invoke<EnrichReportDto>('enrich_results')
}

/** 反查单个目标的主机名。阻塞在后端的独立线程里，最多等 timeoutMs。 */
export async function lookupHostnameFor(
  index: number,
  timeoutMs = 2000,
): Promise<HostnameLookupDto> {
  return invoke<HostnameLookupDto>('lookup_hostname_for', { index, timeoutMs })
}

// ---------------------------------------------------------------------------
// M7：结果快照
// ---------------------------------------------------------------------------

/** 启动时检查有没有上次没处理完的结果；没有则返回 null。 */
export async function pendingSnapshot(): Promise<PendingSnapshotDto | null> {
  return invoke<PendingSnapshotDto | null>('pending_snapshot')
}

/** 保存快照到指定路径（桌面端走文件对话框拿到路径）。 */
export async function saveSnapshot(path: string): Promise<SnapshotSavedDto> {
  return invoke<SnapshotSavedDto>('save_snapshot', { path })
}

/** 移动端：写到应用可写目录并回报完整路径。 */
export async function saveSnapshotToAppDir(): Promise<SnapshotSavedDto> {
  return invoke<SnapshotSavedDto>('save_snapshot_to_app_dir')
}

/** 用户选择「不恢复」。deleteFile 时顺手删掉自动落盘文件。 */
export async function dismissSnapshot(deleteFile: boolean): Promise<void> {
  await invoke('dismiss_snapshot', { deleteFile })
}

/**
 * 恢复快照。path 为空时读自动落盘的那一份。
 * 恢复是**显式动作**：界面必须在用户确认后调用，不做自动恢复。
 */
export async function restoreSnapshot(path?: string): Promise<SnapshotDto> {
  return invoke<SnapshotDto>('restore_snapshot', { path: path ?? null })
}

export function onProbeEvent(cb: (event: ProbeEventDto) => void): Promise<UnlistenFn> {
  return listen<ProbeEventDto>('probe-event', (ev) => cb(ev.payload))
}
