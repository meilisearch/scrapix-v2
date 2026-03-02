interface EmptyStateProps {
  message: string;
  action?: React.ReactNode;
}

export function EmptyState({ message, action }: EmptyStateProps) {
  return (
    <div className="text-center py-12">
      <div className="space-y-3">
        <p className="text-muted-foreground">{message}</p>
        {action}
      </div>
    </div>
  );
}
