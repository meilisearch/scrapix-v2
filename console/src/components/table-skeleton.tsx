import { Skeleton } from "@/components/ui/skeleton";

interface TableSkeletonProps {
  rows?: number;
  columns?: [string, string, string, string];
}

export function TableSkeleton({
  rows = 5,
  columns = ["h-4 w-32", "h-5 w-16 rounded-full", "h-4 w-24", "h-4 w-20 ml-auto"],
}: TableSkeletonProps) {
  return (
    <div className="space-y-3">
      {Array.from({ length: rows }).map((_, i) => (
        <div key={i} className="flex items-center gap-4 py-2">
          {columns.map((cls, j) => (
            <Skeleton key={j} className={cls} />
          ))}
        </div>
      ))}
    </div>
  );
}
