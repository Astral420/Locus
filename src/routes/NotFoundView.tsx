import React from "react";
import { useNavigate } from "@tanstack/react-router";
import { EmptyState } from "../components/ui/EmptyState";
import { AlertCircle } from "lucide-react";

export const NotFoundView: React.FC = () => {
  const navigate = useNavigate();

  return (
    <div className="flex-1 flex items-center justify-center p-6 bg-bg">
      <EmptyState
        icon={<AlertCircle className="w-8 h-8 text-status-warning" />}
        title="Page Not Found"
        description="The requested view does not exist in the Locus application shell."
        actionLabel="Return to Meetings"
        onAction={() => void navigate({ to: "/" })}
      />
    </div>
  );
};
