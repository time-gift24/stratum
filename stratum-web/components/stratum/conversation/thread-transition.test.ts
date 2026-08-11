import { describe, expect, it } from "vitest"

import {
  initialTransitionState,
  reduceThreadTransition,
  type ThreadSignals,
  type TransitionState,
} from "./thread-transition"

function sig(
  conversationId: string | null | undefined,
  sendVersion: number | undefined,
  isEmpty: boolean
): ThreadSignals {
  return { conversationId, sendVersion, isEmpty }
}

/** 依次提交信号，返回每一步的 state（最后一步为主结果） */
function run(
  steps: ThreadSignals[],
  initial: TransitionState = initialTransitionState
): TransitionState[] {
  const states: TransitionState[] = []
  let current = initial
  for (const signals of steps) {
    current = reduceThreadTransition(current, signals)
    states.push(current)
  }
  return states
}

describe("reduceThreadTransition", () => {
  it("首帧只记录信号，不产生动作", () => {
    const [state] = run([sig(null, 0, true)])
    expect(state.action).toBe("none")
    expect(state.prev).toEqual(sig(null, 0, true))
  })

  it("信号无变化时返回原引用（调用方据此跳过 setState）", () => {
    const [first] = run([sig("a", 0, false)])
    const second = reduceThreadTransition(first, sig("a", 0, false))
    expect(second).toBe(first)
  })

  it("新对话首发：发送与首条消息落地同帧 → forward-flip", () => {
    const states = run([sig(null, 0, true), sig(null, 1, false)])
    expect(states[1].action).toBe("forward-flip")
    expect(states[1].pendingSend).toBe(true)
    expect(states[1].leavingEmpty).toBe(true)
  })

  it("新对话首发：发送先于落地两个提交 → settle 后 forward-flip", () => {
    const states = run([sig(null, 0, true), sig(null, 1, true), sig(null, 1, false)])
    expect(states[1].action).toBe("settle")
    expect(states[1].pendingSend).toBe(true)
    expect(states[2].action).toBe("forward-flip")
  })

  it("首发确立新 id 后 recovering 清空再填充：填充不播第二次正向过场", () => {
    const states = run([
      sig(null, 0, true),
      sig(null, 1, false), // 首发落地，forward-flip
      sig("a", 1, true), // 确立新 id + recovering 清空（同帧）
      sig("a", 1, false), // 恢复填充
    ])
    expect(states[2].action).toBe("settle")
    expect(states[2].suppressFill).toBe(true)
    expect(states[3].action).toBe("settle")
    expect(states[3].suppressFill).toBe(false)
  })

  it("首发确立与恢复填充分帧：id 确立时为空 → 填充同样被抑制", () => {
    const states = run([
      sig(null, 0, true),
      sig(null, 1, true), // 发送（消息未落地）
      sig("a", 1, true), // 确立新 id，仍为空（recovering 窗口）
      sig("a", 1, false), // 恢复填充
    ])
    expect(states[2].action).toBe("settle")
    expect(states[2].suppressFill).toBe(true)
    expect(states[3].action).toBe("settle")
    expect(states[3].action).not.toBe("forward-flip")
  })

  it("历史会话间切换：switch + 切换过场置位，恢复填充被抑制", () => {
    const states = run([
      sig("a", 0, false),
      sig("b", 0, true), // 切换 + 暂态空（同帧）
      sig("b", 0, false), // 恢复填充
    ])
    expect(states[1].action).toBe("switch")
    expect(states[1].switching).toBe(true)
    expect(states[1].suppressFill).toBe(true)
    expect(states[2].action).toBe("settle")
    expect(states[2].suppressFill).toBe(false)
  })

  it("已有会话回到新对话空态：reverse-flip，并清掉残留抑制与发送标记", () => {
    const states = run([
      sig("a", 0, false),
      sig("b", 0, false), // 切换但无暂态空（恢复即时完成，抑制残留）
      sig(null, 0, true), // 点新对话
    ])
    expect(states[1].suppressFill).toBe(true)
    expect(states[2].action).toBe("reverse-flip")
    expect(states[2].suppressFill).toBe(false)
  })

  it("切换后回到新对话再发首条：正向过场不被残留抑制误杀", () => {
    const states = run([
      sig("a", 0, false),
      sig("b", 0, false), // 切换，抑制残留
      sig(null, 0, true), // 回新对话，reverse 清除抑制
      sig(null, 1, false), // 首发落地
    ])
    expect(states[3].action).toBe("forward-flip")
  })

  it("id 清空与 items 清空不同帧时仍能播反向过场", () => {
    const states = run([
      sig("a", 0, false),
      sig(null, 0, false), // 先清 id（switch）
      sig(null, 0, true), // 再清空（同一会话视图内翻转到空）
    ])
    expect(states[1].action).toBe("switch")
    expect(states[2].action).toBe("reverse-flip")
  })

  it("已有会话内发送（无翻转）→ settle，不影响后续状态", () => {
    const states = run([sig("a", 0, false), sig("a", 1, false)])
    expect(states[1].action).toBe("settle")
    expect(states[1].pendingSend).toBe(false)
  })

  it("新对话内未发送就切到历史会话：按普通切换处理（非首发确立）", () => {
    const states = run([sig(null, 0, true), sig("a", 0, true)])
    expect(states[1].action).toBe("switch")
    expect(states[1].switching).toBe(true)
  })
})
