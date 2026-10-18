import { useState } from "react";

type DeletionMutation<Preview> = {
	isPending: boolean;
	mutate: (
		subjectId: string,
		callbacks: {
			onSuccess: (preview: Preview) => void;
			onError: (cause: Error) => void;
		},
	) => void;
};

type RemoveMutation = {
	isPending: boolean;
	mutate: (
		variables: { id: string; confirmationToken: string },
		callbacks: { onSuccess: () => void; onError: (cause: Error) => void },
	) => void;
};

/**
 * Shared preview-then-confirm state machine behind Permanent Track Deletion
 * and Album deletion: open loads a preview for the subject, confirm sends the
 * preview's token. Preview failures show inline; the remove mutation owns its
 * own user feedback.
 */
export function useDeletionFlow<
	Subject extends { id: string },
	Preview extends { confirmationToken: string },
>({
	preview: previewMutation,
	remove,
	onDeleted,
}: {
	preview: DeletionMutation<Preview>;
	remove: RemoveMutation;
	onDeleted?: (subject: Subject) => void;
}) {
	const [subject, setSubject] = useState<Subject | null>(null);
	const [preview, setPreview] = useState<Preview | null>(null);
	const [error, setError] = useState<string | null>(null);

	const reset = () => {
		setSubject(null);
		setPreview(null);
		setError(null);
	};
	const open = (selected: Subject) => {
		setSubject(selected);
		setPreview(null);
		setError(null);
		previewMutation.mutate(selected.id, {
			onSuccess: setPreview,
			onError: (cause) => setError(cause.message),
		});
	};
	const cancel = () => {
		if (!remove.isPending) reset();
	};
	const confirm = () => {
		if (!subject || !preview) return;
		remove.mutate(
			{ id: subject.id, confirmationToken: preview.confirmationToken },
			{
				onSuccess: () => {
					onDeleted?.(subject);
					reset();
				},
				onError: (cause) => setError(cause.message),
			},
		);
	};

	return {
		subject,
		preview,
		error,
		isLoading: previewMutation.isPending,
		isDeleting: remove.isPending,
		open,
		cancel,
		confirm,
	};
}
