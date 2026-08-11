/**
 * thread-transition —— ConversationThread 过场信号的纯函数归约。
 *
 * 每个提交的（conversationId / sendVersion / isEmpty）信号归约出一份
 * TransitionState（含 action 结论），render 相位（布局/welcome 可见性）
 * 与 GSAP effect 相位（编排播放）共用同一结论——此前两处各自维护
 * prev/ref 镜像，判定漂移曾导致欢迎语二次淡出、切换闪烁等问题。
 *
 * action 语义：
 * - forward-flip：同一会话空 → 有消息（首条消息落地）；composer 滑到底部、
 *   欢迎语原地淡出、消息区淡入
 * - reverse-flip：回到新对话空态（conversationId 为 null）；镜像滑回、
 *   欢迎语淡入
 * - switch：会话切换（非首发确立）；不播位置动画，内容淡入由切换过场接管
 * - settle：其余信号变化（切换途中的暂态空、被抑制的恢复填充、已有会话内
 *   发送等）；清理中断动画残留，直落目标布局
 * - none：信号无变化（仅首帧/普通重渲染；action 保留上次结论但只有
 *   信号变化的提交才会被 effect 消费，不会重播）
 */

export type ThreadSignals = {
  conversationId: string | null | undefined
  sendVersion: number | undefined
  isEmpty: boolean
}

export type TransitionAction =
  | "none"
  | "forward-flip"
  | "reverse-flip"
  | "switch"
  | "settle"

export type TransitionState = {
  /** 上次提交的信号；null = 首帧 */
  prev: ThreadSignals | null
  /** 新对话（无 id）内已发出首条消息；随后的 null → 新 id 是同一对话的确立 */
  pendingSend: boolean
  /** 下一次空 → 非空是恢复填充（切换/首发确立的 recovering），抑制正向过场 */
  suppressFill: boolean
  /** 会话切换过场进行中：新内容保持透明，恢复结束后滚动落底再淡入。
   *  由 switch 提交置位，由淡入完成的回调（函数式更新）清除 */
  switching: boolean
  /** 正向过场期间保持欢迎语渲染，交给 GSAP 原地淡出。
   *  由 forward-flip 提交置位，由过场完成的回调（函数式更新）清除 */
  leavingEmpty: boolean
  /** 最近一次信号变化的归约结论 */
  action: TransitionAction
}

export const initialTransitionState: TransitionState = {
  prev: null,
  pendingSend: false,
  suppressFill: false,
  switching: false,
  leavingEmpty: false,
  action: "none",
}

/**
 * 归约一次提交。信号无变化时原样返回（引用相等），调用方据此跳过
 * setState；有变化时返回携带新结论的 state。
 */
export function reduceThreadTransition(
  state: TransitionState,
  signals: ThreadSignals
): TransitionState {
  const prev = state.prev
  if (prev === null) return { ...state, prev: signals, action: "none" }
  if (
    prev.conversationId === signals.conversationId &&
    prev.sendVersion === signals.sendVersion &&
    prev.isEmpty === signals.isEmpty
  )
    return state

  const conversationChanged = prev.conversationId !== signals.conversationId
  const sendHappened = prev.sendVersion !== signals.sendVersion
  const flipped = prev.isEmpty !== signals.isEmpty
  // 首发确立：新对话内发出首条消息后的 null → 新 id，是同一对话而非切换
  const createFlow =
    conversationChanged &&
    prev.conversationId == null &&
    signals.conversationId != null &&
    state.pendingSend

  const base = {
    prev: signals,
    // pendingSend 只对新对话（无 id）有意义；进入已有会话即消费
    pendingSend:
      signals.conversationId == null
        ? state.pendingSend || sendHappened
        : false,
    // 切换过场由非首发确立的 id 变化触发，淡入完成才清除
    switching:
      conversationChanged && !createFlow ? true : state.switching,
    // 只有新的 forward-flip 才需要保持欢迎语；其余提交维持现状（由过场
    // 完成回调清除），避免流式重渲染中途打掉正在淡出的欢迎语
    leavingEmpty: state.leavingEmpty,
  }

  // 翻转到空且目的地是新对话：反向过场。无论 id 是否同帧变化（点新对话
  // 时 id 清空与 items 清空可能不在同一提交）。同时结束任何恢复窗口，
  // 清掉残留抑制——否则之后真正的首发过场会被误杀
  if (flipped && signals.isEmpty && signals.conversationId == null)
    return { ...base, suppressFill: false, action: "reverse-flip" }

  // 会话切换（非首发确立）：不播位置动画；随后的恢复填充一律抑制。
  // 同帧翻转到空也归这里（暂态空），内容由 switching 过场接管
  if (conversationChanged && !createFlow)
    return { ...base, suppressFill: true, action: "switch" }

  // 同一会话内翻转到空：恢复途中的暂态空（含首发确立后 recovering 清空）
  if (flipped && signals.isEmpty)
    return { ...base, suppressFill: true, action: "settle" }

  // 首发确立落入 recovering 窗口（当前为空）：随后的填充是恢复填充
  const suppressFill =
    createFlow && signals.isEmpty ? true : state.suppressFill

  // 翻转到有消息：被抑制（恢复填充）→ 直落稳态并消费；否则播正向过场
  if (flipped)
    return suppressFill
      ? { ...base, suppressFill: false, action: "settle" }
      : { ...base, suppressFill, leavingEmpty: true, action: "forward-flip" }

  return { ...base, suppressFill, action: "settle" }
}
