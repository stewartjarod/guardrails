import React from 'react';
import { cn } from '@/lib/utils';

// ❌ BAD: Hardcoded colors with no dark: variants
export function UserCard({ name, email, role }: UserCardProps) {
  return (
    <div className="bg-white border border-gray-200 rounded-lg shadow-sm p-6">
      <div className="flex items-center gap-4">
        <div className="w-12 h-12 rounded-full bg-gray-100 flex items-center justify-center">
          <span className="text-gray-600 text-lg font-bold">
            {name[0]}
          </span>
        </div>
        <div>
          <h3 className="text-gray-900 font-semibold text-lg">{name}</h3>
          <p className="text-gray-500 text-sm">{email}</p>
          <span className="text-slate-400 text-xs">{role}</span>
        </div>
      </div>
      <div className="mt-4 pt-4 border-t border-gray-200">
        <button className="bg-slate-900 text-white px-4 py-2 rounded-md hover:bg-slate-800">
          View Profile
        </button>
        <button className="ml-2 border border-gray-300 text-gray-700 px-4 py-2 rounded-md">
          Message
        </button>
      </div>
    </div>
  );
}

// ❌ BAD: Using cn() with hardcoded colors
export function StatusBadge({ status }: { status: string }) {
  return (
    <span
      className={cn(
        "px-2 py-1 rounded-full text-xs font-medium",
        status === 'active' && "bg-green-100 text-green-800",
        status === 'inactive' && "bg-gray-100 text-gray-600",
        status === 'error' && "bg-red-100 text-red-600",
      )}
    >
      {status}
    </span>
  );
}

// ❌ BAD: Alert with hardcoded destructive colors
export function ErrorAlert({ message }: { message: string }) {
  return (
    <div className="bg-red-500 text-white p-4 rounded-md border border-red-600">
      <p className="font-semibold">Error</p>
      <p className="text-sm">{message}</p>
    </div>
  );
}
