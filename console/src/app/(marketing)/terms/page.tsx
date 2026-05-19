import type { Metadata } from "next";

export const metadata: Metadata = {
  title: "Terms of Service — Scrapix",
  description: "Terms and conditions governing the use of Scrapix.",
};

export default function TermsOfServicePage() {
  return (
    <section className="mx-auto max-w-3xl px-6 py-24 md:py-32">
      <h1 className="mb-2 text-3xl font-bold tracking-tight text-white md:text-4xl">
        Terms of Service
      </h1>
      <p className="mb-12 text-sm text-zinc-500">
        Last updated: March 25, 2026
      </p>

      <div className="prose prose-invert prose-zinc max-w-none [&_h2]:mt-10 [&_h2]:mb-4 [&_h2]:text-xl [&_h2]:font-semibold [&_h2]:text-white [&_h3]:mt-6 [&_h3]:mb-2 [&_h3]:text-base [&_h3]:font-medium [&_h3]:text-zinc-200 [&_p]:text-zinc-400 [&_p]:leading-relaxed [&_ul]:text-zinc-400 [&_li]:text-zinc-400">
        <p>
          These Terms of Service (&quot;Terms&quot;) govern your access to and
          use of the Scrapix platform (&quot;Service&quot;) operated by
          Meilisearch SAS, a simplified joint-stock company under French law,
          registered with the Paris Trade and Companies Register under number
          844 156 364, with offices at 52 boulevard de Sebastopol, 75003 Paris,
          France (&quot;Company&quot;, &quot;we&quot;, &quot;us&quot;).
        </p>
        <p>
          By accessing or using the Service, you agree to be bound by these
          Terms. If you do not agree, you may not use the Service.
        </p>

        <h2>1. Description of the Service</h2>
        <p>
          Scrapix is a web crawling, scraping, and search indexing platform. It
          provides APIs and tools for extracting content from web pages,
          crawling websites, mapping site structures, and indexing results into
          Meilisearch. The Service includes a web console, REST API, CLI tool,
          and MCP server.
        </p>

        <h2>2. Account Registration</h2>
        <p>
          To use the Service, you must create an account by providing a valid
          email address and password, or by authenticating via a supported
          third-party provider (Google, GitHub). You are responsible for:
        </p>
        <ul>
          <li>Maintaining the confidentiality of your account credentials and API keys</li>
          <li>All activity that occurs under your account</li>
          <li>Notifying us immediately of any unauthorized access</li>
        </ul>
        <p>
          You must be at least 16 years old to create an account. Accounts
          registered by automated methods (bots) are not permitted.
        </p>

        <h2>3. Acceptable Use</h2>
        <p>You agree to use the Service only for lawful purposes. You must not:</p>
        <ul>
          <li>
            Crawl or scrape websites in violation of their terms of service,
            robots.txt directives, or applicable law
          </li>
          <li>
            Use the Service to collect personal data of individuals without a
            lawful basis under applicable data protection laws
          </li>
          <li>
            Attempt to circumvent rate limits, authentication mechanisms, or
            security measures of the Service
          </li>
          <li>
            Use the Service to launch denial-of-service attacks or otherwise
            interfere with third-party websites or services
          </li>
          <li>
            Distribute malware, phishing content, or other harmful material
            through or using the Service
          </li>
          <li>
            Resell, sublicense, or redistribute access to the Service without
            our written consent
          </li>
          <li>
            Use the Service to engage in any activity that is illegal under
            French, European Union, or applicable local law
          </li>
        </ul>
        <p>
          We reserve the right to suspend or terminate accounts that violate
          these terms, with or without notice.
        </p>

        <h2>4. API Keys and Authentication</h2>
        <p>
          API keys are confidential credentials linked to your account. You are
          solely responsible for securing your API keys. Do not embed API keys
          in client-side code, public repositories, or shared environments. If
          you believe an API key has been compromised, revoke it immediately
          via the console and generate a new one.
        </p>
        <p>
          We are not liable for unauthorized use of your API keys resulting
          from your failure to secure them.
        </p>

        <h2>5. Pricing and Billing</h2>

        <h3>5.1 Credits</h3>
        <p>
          The Service operates on a credit-based billing model. Credits are
          consumed per API request based on the type of operation, content size,
          and features used (e.g., JavaScript rendering, AI extraction). Credit
          prices are listed on our pricing page and may be updated with 30
          days&apos; advance notice.
        </p>

        <h3>5.2 Payment</h3>
        <p>
          Payments are processed by Stripe, Inc. By providing payment
          information, you authorize us to charge your payment method for
          credits purchased or auto-top-up amounts configured. All amounts are
          in US Dollars unless otherwise specified.
        </p>

        <h3>5.3 Refunds</h3>
        <p>
          Unused credits are non-refundable except where required by applicable
          law. If you believe you were charged in error, contact{" "}
          <a href="mailto:billing@meilisearch.com" className="text-indigo-400 hover:text-indigo-300">
            billing@meilisearch.com
          </a>{" "}
          within 30 days of the charge.
        </p>

        <h3>5.4 Free Tier</h3>
        <p>
          New accounts receive an initial credit deposit. The free tier is
          provided as-is with no guaranteed availability or support level. We
          reserve the right to modify or discontinue the free tier at any time.
        </p>

        <h2>6. Intellectual Property</h2>

        <h3>6.1 Our Property</h3>
        <p>
          The Service, including its software, APIs, documentation, branding,
          and user interface, is owned by or licensed to Meilisearch SAS. All
          rights not expressly granted in these Terms are reserved.
        </p>

        <h3>6.2 Your Content</h3>
        <p>
          You retain ownership of the data you submit to or extract through the
          Service. You are solely responsible for ensuring you have the right to
          crawl, scrape, and index the content you process through the Service.
        </p>

        <h3>6.3 Crawled Content</h3>
        <p>
          The Service crawls and processes publicly available web content on your
          behalf. We do not claim ownership of crawled content. You are
          responsible for complying with the intellectual property rights,
          terms of service, and applicable laws governing the content you
          access through the Service.
        </p>

        <h2>7. Data Processing</h2>
        <p>
          Our collection, use, and protection of your personal data is governed
          by our{" "}
          <a href="/privacy" className="text-indigo-400 hover:text-indigo-300">
            Privacy Policy
          </a>
          , which forms an integral part of these Terms.
        </p>
        <p>
          Where you use the Service to process personal data of third parties
          (e.g., by crawling websites that contain personal information), you
          act as the data controller for such data. You are responsible for
          ensuring a valid legal basis for processing under GDPR and applicable
          data protection laws.
        </p>

        <h2>8. Service Availability</h2>
        <p>
          We strive to maintain high availability but do not guarantee
          uninterrupted access to the Service. We may perform scheduled
          maintenance, and the Service may be temporarily unavailable due to
          technical issues, infrastructure updates, or force majeure events. We
          will make reasonable efforts to provide advance notice of planned
          downtime.
        </p>

        <h2>9. Limitation of Liability</h2>
        <p>
          To the maximum extent permitted by applicable law:
        </p>
        <ul>
          <li>
            The Service is provided &quot;as is&quot; and &quot;as
            available&quot; without warranties of any kind, whether express or
            implied, including but not limited to warranties of
            merchantability, fitness for a particular purpose, and
            non-infringement.
          </li>
          <li>
            We do not warrant that the Service will be error-free,
            uninterrupted, or that crawled content will be accurate or complete.
          </li>
          <li>
            In no event shall our total aggregate liability exceed the amount
            you paid to us in the twelve (12) months preceding the event giving
            rise to the claim.
          </li>
          <li>
            We shall not be liable for any indirect, incidental, special,
            consequential, or punitive damages, including loss of profits, data,
            or business opportunities, regardless of the theory of liability.
          </li>
        </ul>
        <p>
          Nothing in these Terms excludes or limits liability that cannot be
          excluded or limited under applicable French or EU law, including
          liability for fraud or gross negligence.
        </p>

        <h2>10. Indemnification</h2>
        <p>
          You agree to indemnify, defend, and hold harmless Meilisearch SAS, its
          officers, directors, employees, and agents from and against any
          claims, liabilities, damages, losses, or expenses (including
          reasonable legal fees) arising from:
        </p>
        <ul>
          <li>Your use of the Service</li>
          <li>Your violation of these Terms</li>
          <li>Your violation of any third-party rights, including intellectual property or privacy rights</li>
          <li>Content you crawl, scrape, or process through the Service</li>
        </ul>

        <h2>11. Termination</h2>

        <h3>11.1 By You</h3>
        <p>
          You may terminate your account at any time by deleting it through the
          console or by contacting{" "}
          <a href="mailto:support@meilisearch.com" className="text-indigo-400 hover:text-indigo-300">
            support@meilisearch.com
          </a>.
          Upon termination, your data will be deleted in accordance with our
          Privacy Policy.
        </p>

        <h3>11.2 By Us</h3>
        <p>
          We may suspend or terminate your access to the Service immediately and
          without prior notice if:
        </p>
        <ul>
          <li>You breach these Terms</li>
          <li>Your use of the Service poses a security risk or may cause harm to other users</li>
          <li>We are required to do so by law or court order</li>
          <li>Your account has been inactive for more than 12 months</li>
        </ul>

        <h3>11.3 Effect of Termination</h3>
        <p>
          Upon termination, your right to use the Service ceases immediately.
          Sections 6, 9, 10, 12, and 13 survive termination.
        </p>

        <h2>12. Governing Law and Dispute Resolution</h2>
        <p>
          These Terms are governed by and construed in accordance with the laws
          of France, without regard to conflict of law principles. Any disputes
          arising from or relating to these Terms or the Service shall be
          subject to the exclusive jurisdiction of the courts of Paris, France.
        </p>
        <p>
          For EU consumers: nothing in these Terms affects your rights under
          mandatory consumer protection laws of your country of residence. You
          may also use the European Commission&apos;s Online Dispute Resolution
          platform.
        </p>

        <h2>13. General Provisions</h2>
        <ul>
          <li>
            <strong className="text-zinc-200">Entire agreement</strong> — These
            Terms, together with the Privacy Policy, constitute the entire
            agreement between you and the Company regarding the Service.
          </li>
          <li>
            <strong className="text-zinc-200">Severability</strong> — If any
            provision of these Terms is found to be unenforceable, the remaining
            provisions remain in full force and effect.
          </li>
          <li>
            <strong className="text-zinc-200">Waiver</strong> — Our failure to
            enforce any right or provision of these Terms shall not constitute a
            waiver of that right.
          </li>
          <li>
            <strong className="text-zinc-200">Assignment</strong> — You may not
            assign your rights under these Terms without our prior written
            consent. We may assign our rights to an affiliate or successor.
          </li>
          <li>
            <strong className="text-zinc-200">Modifications</strong> — We may
            update these Terms from time to time. We will provide at least
            15 days&apos; advance notice before material changes take effect.
            Your continued use of the Service after changes constitutes
            acceptance.
          </li>
        </ul>

        <h2>14. Contact</h2>
        <p>
          Meilisearch SAS<br />
          52 boulevard de Sebastopol, 75003 Paris, France<br />
          Paris Trade and Companies Register: 844 156 364<br />
          Email:{" "}
          <a href="mailto:legal@meilisearch.com" className="text-indigo-400 hover:text-indigo-300">
            legal@meilisearch.com
          </a>
        </p>
      </div>
    </section>
  );
}
