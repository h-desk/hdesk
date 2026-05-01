---
description: 实现→Review→改进的迭代开发工作流，通过多轮质量评审让代码收敛到目标阈值
agent: agent
argument-hint: 描述任务，可附加：迭代 N 次 / 阈值 N 分 / 重点关注 performance|maintainability|security
---

# Review-Driven 迭代开发工作流

任务：${input:task:描述任务，可附加迭代次数、阈值、评审重点，例如：实现搜索功能，迭代 4 次，阈值 9 分}

可附加参数：
- `迭代 N 次`（默认 3，范围 1~5）
- `阈值 N 分`（默认 8，范围 1~10）
- `重点关注 performance / maintainability / security / all`（默认 all）

**示例**:
- `/rw 实现用户搜索功能`
- `/rw 重构权限校验模块，迭代 5 次，阈值 9 分`
- `/rw 优化列表渲染性能，重点关注 performance`

---

## Phase 0: 解析与澄清

### 0.1 解析参数

从 `$ARGUMENTS` 提取：
- `TASK`：任务描述（必须）
- `MAX_ITERATIONS`：最大迭代次数（默认 3）
- `QUALITY_THRESHOLD`：质量阈值（默认 8）
- `REVIEW_FOCUS`：评审重点（默认 all）

### 0.2 模糊度检测（Clarification Gate）

分析任务描述是否存在以下情况：
- 缺少明确的输入/输出定义
- 目标描述使用高度抽象动词（"优化"、"改进"、"重构"）但无具体指标
- 涉及多个可能互相冲突的目标

**若检测到歧义**，列出 2~3 个最关键的不确定点，附带默认处理方案，等待用户确认：

```
在开始前，有几点需要确认：
1. [不确定点]？（默认：[方案A]，如需调整请告知）
2. [不确定点]？（默认：[方案B]，如需调整请告知）
直接回复"确认"即按默认方案执行。
```

**若任务描述清晰**，跳过此步骤，直接进入 Phase 1。

### 0.3 输出启动摘要

```
任务: [TASK]
配置: 最大 [MAX_ITERATIONS] 轮 · 阈值 [QUALITY_THRESHOLD] 分 · 重点 [REVIEW_FOCUS]
评审维度权重: [根据 REVIEW_FOCUS 调整后的权重分配]
开始迭代...
```

---

## Phase 1: 迭代开发循环

对每轮迭代 N（N 从 1 到 MAX_ITERATIONS）：

### Step 1: 构建 Implementer 上下文

向 Implementer Subagent 注入三层结构化上下文：

**固定层**（每轮不变）：
- 原始任务描述和验收标准
- 技术约束（从项目上下文中读取）

**历史层**（N > 1 时提供）：
- 精简修改日志："第1轮新增X，Reviewer指出Y；第2轮修复Y，引入Z"
- 问题首次出现轮次标注（顽固问题需更根本性处理）

**当前层**：
- 本轮需处理的问题清单（已由 Orchestrator 预分类，见下方优先级分类）
- 本轮行为模式指令

### Step 2: 确定 Implementer 行为模式

根据轮次和状态注入不同的行为指令：

- **第 1 轮 → Builder 模式**
  > 优先实现完整性。允许做技术选型和结构设计。自检重点：功能是否全部实现、是否有明显设计缺陷。

- **第 2 轮起 → Surgeon 模式**
  > 最小化变更原则。禁止在问题清单之外做"顺手优化"。必须先输出变更范围声明，再开始修改。自检基于问题清单逐条核对。

- **回滚后 → Recovery 模式**
  > 接收回滚原因分析，必须说明本次方案与上次有何本质不同，再开始实现。

### Step 3: 变更范围声明协议（Surgeon / Recovery 模式）

Implementer 在修改前必须输出：
```
本轮计划修改：
- [文件A] 的 [函数/模块]，原因：[对应问题清单条目]
- [文件B] 的 [函数/模块]，原因：[对应问题清单条目]
```

修改完成后输出实际变更列表。若实际范围超出声明范围，超出部分单独标注，作为下一轮 Reviewer 的重点检查区域。

### Step 4: 派发 Implementer Subagent

```
你是一个软件实现专家。

## 任务
[TASK]

## 当前轮次
第 [N] 轮 / 共 [MAX_ITERATIONS] 轮
行为模式：[Builder / Surgeon / Recovery]

## 上下文
[注入固定层 + 历史层 + 当前层]

## 本轮待处理问题（按优先级）

### Must Fix（必须修复，否则本轮视为失败）
[Orchestrator 预分类的阻断性问题]

### Should Fix（应该修复，影响评分）
[质量性问题]

### Could Fix（可选，当前轮 Must/Should 全部完成后再考虑）
[改进性建议]

## 输出格式

### 变更范围声明（Surgeon/Recovery 模式）
[修改前的计划]

### 实现摘要
[完成的内容]

### 修改文件
| 文件 | 改动类型 | 对应问题 |
|------|----------|----------|

### 自检结果（对照问题清单逐条核对）
| 问题 | 状态 | 说明 |
|------|------|------|
| [Must Fix 条目] | 已解决 / 部分解决 / 未解决 | [原因] |

### 新引入风险（主动声明本轮改动中有潜在风险的地方）
[若无则写"无"]
```

### Step 5: 派发 Architect Reviewer Subagent

**注意：不向 Reviewer 透露上轮具体分数，只提供问题清单和本轮声称修复的内容，防止锚定偏差。**

```
你是一个代码架构师，负责质量评审。

## 评审范围
[本轮修改的文件列表]

## 评审基准
请先对照"满分标准"评估差距，再打分。不要与上一版本比较，要与理想实现比较。

满分标准：功能完整无遗漏、代码清晰可读、无明显性能瓶颈、模块化良好可扩展、
符合项目技术规范、关键路径有测试覆盖。

## 评审维度与权重

[根据 REVIEW_FOCUS 动态调整权重]

**REVIEW_FOCUS=all（默认）**
| 维度 | 权重 |
|------|------|
| 功能完整性 | 25% |
| 代码质量 | 20% |
| 性能影响 | 20% |
| 可维护性 | 15% |
| 技术规范合规性 | 15% |
| 测试覆盖 | 5% |

**REVIEW_FOCUS=performance**
| 维度 | 权重 |
|------|------|
| 性能影响 | 35% |
| 功能完整性 | 25% |
| 测试覆盖 | 15% |
| 代码质量 | 15% |
| 可维护性 | 5% |
| 技术规范合规性 | 5% |

**REVIEW_FOCUS=maintainability**
| 维度 | 权重 |
|------|------|
| 可维护性 | 35% |
| 代码质量 | 25% |
| 技术规范合规性 | 20% |
| 功能完整性 | 15% |
| 性能影响 | 3% |
| 测试覆盖 | 2% |

**REVIEW_FOCUS=security**
| 维度 | 权重 |
|------|------|
| 技术规范合规性 | 35% |
| 功能完整性 | 25% |
| 代码质量 | 20% |
| 可维护性 | 10% |
| 测试覆盖 | 7% |
| 性能影响 | 3% |

## 上轮遗留问题（本轮声称已修复）
[Implementer 的自检结果]

## 额外重点检查区域
[Implementer 声明的新引入风险 + 超出变更范围声明的部分]

## 输出格式（JSON，如无法输出 JSON 则用 Markdown 表格替代）

{
  "score": 7.5,
  "dimensions": {
    "functionality": 8,
    "code_quality": 7,
    "performance": 7,
    "maintainability": 8,
    "compliance": 7,
    "test_coverage": 6
  },
  "issues": [
    {
      "id": "I001",
      "severity": "Blocking",
      "domain": "Correctness",
      "file": "path/to/file",
      "line": 123,
      "description": "...",
      "suggestion": "..."
    },
    {
      "id": "I002",
      "severity": "Degrading",
      "domain": "Performance",
      "file": "path/to/file",
      "line": 45,
      "description": "...",
      "suggestion": "..."
    }
  ],
  "verified_fixes": ["问题A已修复", "问题B已修复"],
  "unverified_claimed_fixes": ["声称修复了C，但代码中未见对应改动"],
  "verdict": "NEEDS_WORK"
}

## Verdict 判定规则
- APPROVED：无 Blocking 问题，综合评分 >= 阈值
- NEEDS_WORK：有 Degrading 问题，可通过迭代修复
- BLOCKING_ISSUES：有 Blocking 问题，必须立即修复（不计入迭代次数）
```

### Step 6: Orchestrator 收敛判断

收到 Reviewer 报告后：

```
IF verdict == BLOCKING_ISSUES:
    → 不计入迭代次数，立即派发新 Implementer（Recovery 模式），只处理 Blocking 问题

ELIF score >= QUALITY_THRESHOLD AND verdict == APPROVED:
    → 进入伪收敛检测（Step 7）

ELIF score < 上轮 score - 0.5（质量倒退）:
    → 回滚代码到上一版本
    → 派发新 Implementer（Recovery 模式），附带回滚原因分析

ELIF 连续 2 轮 score 变化 < 0.3 AND 问题清单无变化（震荡）:
    → 在下一轮 Implementer 上下文中增加：历史改动对比 + 要求从根本原因分析入手

ELIF N >= MAX_ITERATIONS:
    → 进入 Phase 2（附带遗留问题）

ELSE:
    → 对问题清单做 Must/Should/Could 分类，进入下一轮
```

**Must/Should/Could 分类规则**：
- **Must Fix**：`Blocking` 级别的所有问题
- **Should Fix**：`Degrading` 级别，且属于当前 REVIEW_FOCUS 域的问题
- **Could Fix**：`Degrading` 级别的其他问题 + 所有 `Optimizing` 级别问题

### Step 7: 伪收敛检测（达到阈值时执行）

在宣布收敛前，执行以下校验：

1. **测试完整性守护**：本轮测试覆盖率或测试用例数量是否低于初始轮次？若是，不允许收敛，将测试覆盖补充列为 Must Fix。

2. **问题消失检查**：对比历史问题清单，若某个 Blocking/Degrading 问题在没有对应代码修改的情况下"消失"，强制 Reviewer 复核该问题。

3. **变更合理性检查**：若本轮评分提升 > 1.0 但实际代码变更极小（只改注释/变量名），标记为可疑，要求 Reviewer 针对核心逻辑重新打分。

全部通过 → 确认收敛，进入 Phase 2。

### 轮次进度输出（每轮结束时）

```
[第 N 轮完成] 评分: X.X 分 · 修复: N 个问题 · 遗留: N 个问题 · 下一轮聚焦: [...]
```

---

## Phase 2: 验收与报告

### 2.1 最终验收

- 核查所有 Must Fix 问题是否已解决
- 确认满足原始任务的验收标准

### 2.2 生成迭代报告

报告采用倒金字塔结构：

```markdown
## 结论
[一句话：是否建议接受本次改动，最终质量评估]

## 评分走势
迭代 1: [====      ] X.X 分
迭代 2: [======    ] X.X 分 (+X.X)
迭代 3: [========  ] X.X 分 (+X.X) ✅ 达到阈值

## 问题修复追踪
| 问题 | 严重度·域 | 发现于 | 修复于 | 状态 |
|------|-----------|--------|--------|------|
| ... | Blocking·Correctness | 第1轮 | 第2轮 | ✅ |
| ... | Degrading·Performance | 第2轮 | - | ⏳ 遗留 |

## 各维度评分变化
| 维度 | 第1轮 | 第2轮 | 第3轮 | 趋势 |
|------|-------|-------|-------|------|

## 遗留问题与后续建议
[Could Fix 积压清单，供后续处理]
```

---

## 约束

1. **不跳过澄清**：任务描述模糊时必须先澄清，不直接开始实现
2. **不跳过 Review**：每轮实现后必须派发 Reviewer，不允许省略
3. **不透露分数给 Reviewer**：只传问题清单，防锚定偏差
4. **Blocking 问题不计迭代次数**：必须修复后才开始正式迭代
5. **Surgeon 模式必须写变更范围声明**：不允许省略

