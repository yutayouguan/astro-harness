import { CircleCheck, LoaderCircle } from "lucide-react";

type Props = {
  inProgress: boolean;
};

export default function SessionStatusIcon({ inProgress }: Props) {
  return (
    <span className="session-status-icon" aria-hidden>
      {inProgress ? (
        <LoaderCircle
          className="session-status-icon-spin"
          size={14}
          strokeWidth={2.2}
        />
      ) : (
        <CircleCheck
          className="session-status-icon-complete"
          size={14}
          strokeWidth={2}
        />
      )}
    </span>
  );
}
