import type { ReactNode } from "react";

/**
 * 终端命令卡：深处签名组件。
 * 引用终端器物感（8px 圆角、窗控点），命令前缀 $ 用印章红。
 */
type TerminalCardProps = {
  title: string;
  note: string;
  lines: string[];
};

export function TerminalCard({
  title,
  note,
  lines,
}: TerminalCardProps): ReactNode {
  return (
    <figure className="rounded-lg bg-abyss-raise shadow-card-deep">
      <figcaption className="flex items-center gap-2 px-5 pt-4 pb-3">
        <span className="flex gap-1.5" aria-hidden>
          <i className="h-2.5 w-2.5 rounded-full bg-bone/15" />
          <i className="h-2.5 w-2.5 rounded-full bg-bone/15" />
          <i className="h-2.5 w-2.5 rounded-full bg-bone/15" />
        </span>
        <span className="tracking-label ml-2 font-mono text-xs text-fog uppercase">
          {title}
        </span>
      </figcaption>
      <div className="px-5 pb-5">
        <pre
          translate="no"
          className="overflow-x-auto font-mono text-sm leading-8 text-bone [mask-image:linear-gradient(90deg,#000_92%,transparent)]"
        >
          {lines.map((line) => (
            <code key={line} className="block">
              <span className="mr-3 text-seal">$</span>
              {line}
            </code>
          ))}
        </pre>
        <p className="mt-3 font-mono text-xs text-fog"># {note}</p>
      </div>
    </figure>
  );
}
