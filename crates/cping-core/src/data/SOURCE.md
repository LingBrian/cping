# OUI 数据来源

本目录下的 `*.txt` 由 `tools/oui-gen/gen_oui.py` 从 **IEEE 官方注册表**生成，
请勿手工编辑（下次重新生成会覆盖）。

生成日期：2026-10-14

| 表 | 注册表 | 位宽 | 条目数 | 数据大小 | 来源 |
| --- | --- | --- | --- | --- | --- |
| `ma_l` | MA-L | 24 bit | 40325 | 1225382 字节 | https://standards-oui.ieee.org/oui/oui.csv |
| `ma_m` | MA-M | 28 bit | 6633 | 227538 字节 | https://standards-oui.ieee.org/oui28/mam.csv |
| `ma_s` | MA-S | 36 bit | 7232 | 226097 字节 | https://standards-oui.ieee.org/oui36/oui36.csv |

合计：54190 条，1679017 字节。

记录格式：每行 `<十六进制前缀>\t<厂商名>`，按前缀升序，行尾 `\n`，UTF-8。
查询按 IEEE 规则做最长前缀匹配：MA-S(36bit) > MA-M(28bit) > MA-L(24bit)。

重新生成：

```bash
python tools/oui-gen/gen_oui.py --src <csv 所在目录>
```
