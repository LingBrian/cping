#!/usr/bin/env python3
"""从 IEEE 官方注册表生成 cping 的离线 OUI 数据表。

数据来源（IEEE 公开注册表，无需登录）：
    MA-L : https://standards-oui.ieee.org/oui/oui.csv       (~3.9 MB, 24bit)
    MA-M : https://standards-oui.ieee.org/oui28/mam.csv     (~0.8 MB, 28bit)
    MA-S : https://standards-oui.ieee.org/oui36/oui36.csv   (~0.7 MB, 36bit)

产出（写入 cping-core 源码目录，由 include_str! 编译期嵌入）：
    crates/cping-core/src/data/oui_ma_l.txt   每行 "AABBCC\\t厂商名"
    crates/cping-core/src/data/oui_ma_m.txt   每行 "AABBCCD\\t厂商名"
    crates/cping-core/src/data/oui_ma_s.txt   每行 "AABBCCDEF\\t厂商名"
    crates/cping-core/src/data/SOURCE.md      来源与条目数（供审计）

记录格式刻意保持极简：按 assignment 升序、以 \\n 分隔、前缀与厂商名用单个
制表符分隔。这样查询侧可以直接对整块字符串做「按行边界跳转的二分查找」，
零分配、零额外索引表。

用法：
    python gen_oui.py --src <包含三个 csv 的目录> [--out <cping-core/src/data>]
    python gen_oui.py                     # 自动下载到临时目录后再生成

下载提示：本机若直连较慢，可用 curl 并行分段下载（见仓库 docs/testing.md），
再以 --src 指向本地目录。
"""
from __future__ import annotations

import argparse
import csv
import io
import os
import sys
import tempfile
import urllib.request

SOURCES = {
    "oui_ma_l": ("MA-L", "oui.csv", "https://standards-oui.ieee.org/oui/oui.csv", 6),
    "oui_ma_m": ("MA-M", "mam.csv", "https://standards-oui.ieee.org/oui28/mam.csv", 7),
    "oui_ma_s": ("MA-S", "oui36.csv", "https://standards-oui.ieee.org/oui36/oui36.csv", 9),
}

UA = {"User-Agent": "Mozilla/5.0 (compatible; cping-oui-gen/1.0)"}
HEX = set("0123456789ABCDEF")


def download(url: str, dest: str) -> str:
    """下载注册表（只用 GET，避免部分 WAF 对 HEAD 返回 418）。"""
    print(f"  下载 {url}", flush=True)
    req = urllib.request.Request(url, headers=UA)  # GET
    with urllib.request.urlopen(req, timeout=300) as r, open(dest, "wb") as f:
        while True:
            chunk = r.read(1 << 20)
            if not chunk:
                break
            f.write(chunk)
    return dest


def normalize_name(name: str) -> str:
    """折叠空白并去首尾空格——IEEE 数据里存在双空格与尾随空格。"""
    return " ".join(name.split())


def parse_registry(path: str, expect_len: int, label: str):
    """解析一张注册表，返回 {assignment: vendor}；发现问题立即报错而不是静默。"""
    rows = {}
    duplicates = 0
    bad = 0
    with open(path, "r", encoding="utf-8-sig", newline="") as f:
        reader = csv.DictReader(f)
        need = {"Assignment", "Organization Name"}
        if not need.issubset(reader.fieldnames or []):
            raise SystemExit(f"{path}: 表头缺字段，实际为 {reader.fieldnames}")
        for row in reader:
            raw = (row.get("Assignment") or "").strip().upper()
            vendor = normalize_name(row.get("Organization Name") or "")
            if len(raw) != expect_len or any(c not in HEX for c in raw):
                bad += 1
                continue
            if not vendor:
                bad += 1
                continue
            if raw in rows:
                duplicates += 1
                continue
            rows[raw] = vendor
    print(
        f"  {label}: 有效 {len(rows)} 条 / 跳过 {bad} 条 / 重复 {duplicates} 条",
        flush=True,
    )
    if not rows:
        raise SystemExit(f"{path}: 解析结果为空，拒绝生成空表")
    return rows


def write_table(rows: dict, out_path: str) -> int:
    """按 assignment 升序写出制表符分隔的行；返回字节数。"""
    keys = sorted(rows)  # 定长十六进制，字典序即数值序
    buf = io.StringIO()
    for k in keys:
        vendor = rows[k]
        # 制表符/换行会破坏行结构，替换为空格（IEEE 数据中极罕见）
        vendor = vendor.replace("\t", " ").replace("\n", " ").replace("\r", " ")
        buf.write(f"{k}\t{vendor}\n")
    data = buf.getvalue().encode("utf-8")
    with open(out_path, "wb") as f:
        f.write(data)
    return len(data)


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--src", help="已下载的 csv 所在目录（缺省则自动下载）")
    ap.add_argument("--out", default=None, help="输出目录（默认 cping-core/src/data）")
    args = ap.parse_args()

    here = os.path.dirname(os.path.abspath(__file__))
    repo = os.path.abspath(os.path.join(here, "..", ".."))
    out_dir = args.out or os.path.join(repo, "crates", "cping-core", "src", "data")
    os.makedirs(out_dir, exist_ok=True)

    tmp = None
    src_dir = args.src
    if not src_dir:
        tmp = tempfile.mkdtemp(prefix="oui-")
        src_dir = tmp

    summary = []
    total_bytes = 0
    for stem, (label, fname, url, exp_len) in SOURCES.items():
        path = os.path.join(src_dir, fname)
        if not os.path.exists(path) or os.path.getsize(path) == 0:
            if tmp is None:
                raise SystemExit(f"{path} 不存在或为空；请下载后再用 --src 指定")
            path = download(url, path)
        rows = parse_registry(path, exp_len, label)
        out_path = os.path.join(out_dir, f"{stem}.txt")
        size = write_table(rows, out_path)
        total_bytes += size
        summary.append((label, fname, url, exp_len, len(rows), size))
        print(f"  写出 {out_path} ({size} 字节)", flush=True)

    # SOURCE.md：把来源、条目数、抓取日期固化下来，方便审计与后续更新
    from datetime import date

    md = [
        "# OUI 数据来源",
        "",
        "本目录下的 `*.txt` 由 `tools/oui-gen/gen_oui.py` 从 **IEEE 官方注册表**生成，",
        "请勿手工编辑（下次重新生成会覆盖）。",
        "",
        f"生成日期：{date.today().isoformat()}",
        "",
        "| 表 | 注册表 | 位宽 | 条目数 | 数据大小 | 来源 |",
        "| --- | --- | --- | --- | --- | --- |",
    ]
    for label, fname, url, bits, count, size in summary:
        md.append(
            f"| `{label.lower().replace('-', '_')}` | {label} | {bits * 4} bit | "
            f"{count} | {size} 字节 | {url} |"
        )
    md += [
        "",
        f"合计：{sum(s[4] for s in summary)} 条，{total_bytes} 字节。",
        "",
        "记录格式：每行 `<十六进制前缀>\\t<厂商名>`，按前缀升序，行尾 `\\n`，UTF-8。",
        "查询按 IEEE 规则做最长前缀匹配：MA-S(36bit) > MA-M(28bit) > MA-L(24bit)。",
        "",
        "重新生成：",
        "",
        "```bash",
        "python tools/oui-gen/gen_oui.py --src <csv 所在目录>",
        "```",
        "",
    ]
    with open(os.path.join(out_dir, "SOURCE.md"), "w", encoding="utf-8", newline="\n") as f:
        f.write("\n".join(md))

    print(f"\n合计 {sum(s[4] for s in summary)} 条，{total_bytes} 字节 → {out_dir}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
