"use client";

import { useState } from "react";
import { usePathname } from "next/navigation";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { MessageSquare, Smile, Meh, Frown, HelpCircle } from "lucide-react";
import { toast } from "sonner";
import { cn } from "@/lib/utils";
import { useMe } from "@/lib/hooks";

type FeedbackType = "happy" | "neutral" | "sad" | "confused" | null;

const feedbackTopics = [
  { value: "bug", label: "Bug Report" },
  { value: "feature", label: "Feature Request" },
  { value: "other", label: "Other" },
];

export function FeedbackDialog({ onNavigate }: { onNavigate?: () => void }) {
  const { data: user } = useMe();
  const pathname = usePathname();
  const [open, setOpen] = useState(false);
  const [topic, setTopic] = useState("");
  const [title, setTitle] = useState("");
  const [feedback, setFeedback] = useState("");
  const [selectedReaction, setSelectedReaction] =
    useState<FeedbackType>(null);
  const [isSubmitting, setIsSubmitting] = useState(false);

  const handleSubmit = async () => {
    if (!topic || !title.trim() || !feedback.trim()) {
      toast.error("Please select a topic, provide a title and feedback");
      return;
    }

    setIsSubmitting(true);
    try {
      const response = await fetch("/api/feedback", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          topic,
          title: title.trim(),
          feedback,
          reaction: selectedReaction,
          email: user?.email,
          path: pathname,
        }),
      });

      if (response.ok) {
        const data = await response.json();
        toast.success("Thank you for your feedback!", {
          description: data.issueIdentifier
            ? `Issue ${data.issueIdentifier} created`
            : undefined,
        });
        setOpen(false);
        onNavigate?.();
        setTopic("");
        setTitle("");
        setFeedback("");
        setSelectedReaction(null);
      } else {
        toast.error("Failed to submit feedback");
      }
    } catch {
      toast.error("An error occurred while submitting feedback");
    } finally {
      setIsSubmitting(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild>
        <Button
          variant="ghost"
          className="w-full justify-start text-muted-foreground"
        >
          <MessageSquare className="mr-3 h-4 w-4" />
          Feedback
        </Button>
      </DialogTrigger>
      <DialogContent className="sm:max-w-[500px]">
        <DialogHeader>
          <DialogTitle>Send Feedback</DialogTitle>
          <DialogDescription>
            Help us improve by sharing your thoughts
          </DialogDescription>
        </DialogHeader>
        <div className="space-y-4">
          <Select value={topic} onValueChange={setTopic}>
            <SelectTrigger>
              <SelectValue placeholder="Select a topic..." />
            </SelectTrigger>
            <SelectContent>
              {feedbackTopics.map((t) => (
                <SelectItem key={t.value} value={t.value}>
                  {t.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>

          <Input
            placeholder="Title"
            value={title}
            onChange={(e) => setTitle(e.target.value)}
          />

          <div className="space-y-2">
            <Textarea
              placeholder="Your feedback..."
              value={feedback}
              onChange={(e) => setFeedback(e.target.value)}
              className="min-h-[120px] resize-none"
            />
            <p className="text-xs text-muted-foreground text-right">
              Markdown supported
            </p>
          </div>

          <div className="flex items-center justify-between">
            <div className="flex gap-2">
              {(
                [
                  { type: "happy", Icon: Smile },
                  { type: "neutral", Icon: Meh },
                  { type: "sad", Icon: Frown },
                  { type: "confused", Icon: HelpCircle },
                ] as const
              ).map(({ type, Icon }) => (
                <Button
                  key={type}
                  type="button"
                  variant="ghost"
                  size="sm"
                  className={cn(
                    "p-2",
                    selectedReaction === type && "bg-accent"
                  )}
                  onClick={() => setSelectedReaction(type)}
                >
                  <Icon className="h-5 w-5" />
                </Button>
              ))}
            </div>

            <Button
              onClick={handleSubmit}
              disabled={
                isSubmitting || !topic || !title.trim() || !feedback.trim()
              }
            >
              {isSubmitting ? "Sending..." : "Send"}
            </Button>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
}
