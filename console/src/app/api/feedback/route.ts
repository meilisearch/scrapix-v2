import { NextRequest, NextResponse } from "next/server";

const LINEAR_API_URL = "https://api.linear.app/graphql";
const LINEAR_TEAM_ID = "b223ee3a-3d9f-4cfd-bf55-895edc414d6c"; // SCR (Scrapix)

interface FeedbackPayload {
  topic: string;
  title: string;
  feedback: string;
  reaction: "happy" | "neutral" | "sad" | "confused" | null;
  email?: string;
  path?: string;
}

const topicLabels: Record<string, string> = {
  bug: "Bug Report",
  feature: "Feature Request",
  other: "Other",
};

const reactionEmojis: Record<string, string> = {
  happy: "😊",
  neutral: "😐",
  sad: "😢",
  confused: "🤔",
};

async function linearRequest<T>(
  apiKey: string,
  query: string,
  variables?: Record<string, unknown>
): Promise<T> {
  const response = await fetch(LINEAR_API_URL, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      Authorization: apiKey,
    },
    body: JSON.stringify({ query, variables }),
  });

  const json = await response.json();

  if (json.errors?.length) {
    throw new Error(json.errors[0].message);
  }

  return json.data as T;
}

export async function POST(request: NextRequest) {
  try {
    const linearApiKey = process.env.LINEAR_API_KEY;

    if (!linearApiKey) {
      console.error("Missing LINEAR_API_KEY");
      return NextResponse.json(
        { error: "Feedback service not configured" },
        { status: 500 }
      );
    }

    const body: FeedbackPayload = await request.json();
    const { topic, title: userTitle, feedback, reaction, email, path } = body;

    if (!topic || !userTitle?.trim() || !feedback?.trim()) {
      return NextResponse.json(
        { error: "Topic, title, and feedback are required" },
        { status: 400 }
      );
    }

    const topicLabel = topicLabels[topic] || topic;
    const reactionText = reaction ? reactionEmojis[reaction] : null;
    const title = `[${topicLabel}] ${userTitle.trim()}`;

    const descriptionParts = [feedback];
    descriptionParts.push("\n---");
    if (reactionText) {
      descriptionParts.push(`**Reaction:** ${reactionText}`);
    }
    if (path) {
      descriptionParts.push(`**Page:** \`${path}\``);
    }
    if (email) {
      descriptionParts.push(`**Submitted by:** ${email}`);
    }
    const description = descriptionParts.join("\n");

    const issueResult = await linearRequest<{
      issueCreate: {
        success: boolean;
        issue: { id: string; identifier: string; url: string };
      };
    }>(
      linearApiKey,
      `
      mutation($input: IssueCreateInput!) {
        issueCreate(input: $input) {
          success
          issue {
            id
            identifier
            url
          }
        }
      }
    `,
      {
        input: {
          teamId: LINEAR_TEAM_ID,
          title,
          description,
          priority: 4,
        },
      }
    );

    if (!issueResult.issueCreate.success) {
      return NextResponse.json(
        { error: "Failed to create feedback issue" },
        { status: 500 }
      );
    }

    const issue = issueResult.issueCreate.issue;

    return NextResponse.json({
      success: true,
      issueIdentifier: issue.identifier,
      issueUrl: issue.url,
    });
  } catch (error) {
    console.error("Feedback submission error", error);
    return NextResponse.json(
      { error: "An error occurred while submitting feedback" },
      { status: 500 }
    );
  }
}
