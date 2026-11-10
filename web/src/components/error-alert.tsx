import { CircleAlert } from "lucide-react";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";

interface ErrorAlertProps {
  title: string;
  message: string;
}

/** 错误提示：中文说明 + 后端原始信息（原样保留英文内容）。 */
export function ErrorAlert({ title, message }: ErrorAlertProps) {
  return (
    <Alert variant="destructive" className="border-destructive/30">
      <CircleAlert />
      <AlertTitle>{title}</AlertTitle>
      <AlertDescription>
        <span className="block font-mono text-xs break-words whitespace-pre-wrap">{message}</span>
      </AlertDescription>
    </Alert>
  );
}
