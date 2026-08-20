/** Shared empty/search-zero state for Stratum list surfaces. */
export function EmptyState({
  title,
  description,
  children,
}: {
  title: string
  description: string
  children?: React.ReactNode
}) {
  return (
    <div className="rounded-2xl border border-dashed border-border p-7 sm:p-10">
      <h2 className="font-semibold">{title}</h2>
      <p className="mt-2 max-w-[65ch] text-sm leading-6 text-muted-foreground">
        {description}
      </p>
      {children ? (
        <div className="mt-4 flex flex-wrap items-center gap-2">{children}</div>
      ) : null}
    </div>
  )
}
