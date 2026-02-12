import React from 'react';
import { cn } from '@/lib/utils';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';

// ✅ GOOD: Uses shadcn semantic tokens throughout
export function UserCard({ name, email, role }: UserCardProps) {
  return (
    <Card>
      <CardHeader>
        <div className="flex items-center gap-4">
          <div className="w-12 h-12 rounded-full bg-muted flex items-center justify-center">
            <span className="text-muted-foreground text-lg font-bold">
              {name[0]}
            </span>
          </div>
          <div>
            <CardTitle>{name}</CardTitle>
            <p className="text-muted-foreground text-sm">{email}</p>
            <span className="text-muted-foreground text-xs">{role}</span>
          </div>
        </div>
      </CardHeader>
      <CardContent>
        <div className="border-t border-border pt-4">
          <Button>View Profile</Button>
          <Button variant="outline" className="ml-2">Message</Button>
        </div>
      </CardContent>
    </Card>
  );
}

// ✅ GOOD: Status badge using semantic tokens
export function StatusBadge({ status }: { status: string }) {
  return (
    <Badge
      variant={status === 'error' ? 'destructive' : 'secondary'}
      className={cn(
        "text-xs",
        status === 'active' && "bg-primary text-primary-foreground",
      )}
    >
      {status}
    </Badge>
  );
}

// ✅ GOOD: Alert using destructive semantic token
export function ErrorAlert({ message }: { message: string }) {
  return (
    <div className="bg-destructive text-destructive-foreground p-4 rounded-md border border-destructive">
      <p className="font-semibold">Error</p>
      <p className="text-sm">{message}</p>
    </div>
  );
}
