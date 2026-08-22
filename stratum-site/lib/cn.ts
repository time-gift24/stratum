import { extendTailwindMerge } from "tailwind-merge";

/**
 * 拼接并合并 className。后传入的冲突类胜出（shadcn 惯例），
 * 调用方经 className 覆盖公共组件样式因此是可靠的。
 *
 * 注意：自定义字号 token（--text-ui 等）生成的 text-ui / text-lead …
 * 默认会被 tailwind-merge 误判为 text-{color}，与 text-paper 等颜色类
 * 互相吞掉。必须把它们显式注册进 font-size 组。
 */
const twMerge = extendTailwindMerge({
  extend: {
    classGroups: {
      "font-size": [
        { text: ["ui", "lead", "input", "display", "display-sm", "headline"] },
      ],
    },
  },
});

export function cn(
  ...classes: Array<string | false | null | undefined>
): string {
  return twMerge(...classes);
}
