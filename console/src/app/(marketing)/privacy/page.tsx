import type { Metadata } from "next";

export const metadata: Metadata = {
  title: "Privacy Policy — Scrapix",
  description: "How Scrapix collects, uses, and protects your data.",
};

export default function PrivacyPolicyPage() {
  return (
    <section className="mx-auto max-w-3xl px-6 py-24 md:py-32">
      <h1 className="mb-2 text-3xl font-bold tracking-tight text-white md:text-4xl">
        Privacy Policy
      </h1>
      <p className="mb-12 text-sm text-zinc-500">
        Last updated: March 25, 2026
      </p>

      <div className="prose prose-invert prose-zinc max-w-none [&_h2]:mt-10 [&_h2]:mb-4 [&_h2]:text-xl [&_h2]:font-semibold [&_h2]:text-white [&_h3]:mt-6 [&_h3]:mb-2 [&_h3]:text-base [&_h3]:font-medium [&_h3]:text-zinc-200 [&_p]:text-zinc-400 [&_p]:leading-relaxed [&_ul]:text-zinc-400 [&_li]:text-zinc-400">
        <h2>1. Definitions</h2>
        <ul>
          <li>
            <strong className="text-zinc-200">&quot;Company&quot;</strong>,
            &quot;we&quot;, &quot;us&quot;, &quot;our&quot; refers to Meilisearch
            SAS, a simplified joint-stock company under French law, registered
            with the Paris Trade and Companies Register under number 844 156 364,
            with offices at 52 boulevard de Sebastopol, 75003 Paris, France.
          </li>
          <li>
            <strong className="text-zinc-200">&quot;Service&quot;</strong> refers
            to the Scrapix web crawling, scraping, and search indexing platform
            operated by the Company, accessible at scrapix.meilisearch.com and
            api.scrapix.meilisearch.com.
          </li>
          <li>
            <strong className="text-zinc-200">&quot;Personal Data&quot;</strong>{" "}
            means any information relating to an identified or identifiable
            natural person as defined by GDPR Article 4(1).
          </li>
          <li>
            <strong className="text-zinc-200">&quot;Usage Data&quot;</strong>{" "}
            refers to data collected automatically through use of the Service,
            such as API request metadata, page visit timestamps, and device
            information.
          </li>
          <li>
            <strong className="text-zinc-200">&quot;User&quot;</strong>,
            &quot;you&quot;, &quot;your&quot; refers to any individual or legal
            entity accessing or using the Service.
          </li>
        </ul>

        <h2>2. Data Controller</h2>
        <p>
          Meilisearch SAS is the data controller within the meaning of
          GDPR Article 4(7) for Personal Data collected through the Service.
          For any questions regarding data processing, contact us at{" "}
          <a href="mailto:privacy@meilisearch.com" className="text-indigo-400 hover:text-indigo-300">
            privacy@meilisearch.com
          </a>.
        </p>

        <h2>3. Data We Collect</h2>

        <h3>3.1 Account Data</h3>
        <p>
          When you create an account, we collect your email address, full name
          (optional), and a cryptographic hash of your password (Argon2). If you
          authenticate via Google or GitHub, we receive your email address,
          display name, and provider-specific user ID. We do not store OAuth
          access tokens beyond the initial authentication exchange.
        </p>

        <h3>3.2 Usage Data</h3>
        <p>
          We automatically collect data about your use of the Service,
          including: API requests (timestamps, endpoints, response codes, content
          size), crawl job configurations, URLs crawled and their metadata, and
          bandwidth consumed. This data is used for billing, analytics, rate
          limiting, and service improvement.
        </p>

        <h3>3.3 Payment Data</h3>
        <p>
          Payment processing is handled by Stripe, Inc. We do not store credit
          card numbers or bank account details. We retain your Stripe customer
          ID and payment method identifiers for billing purposes.
        </p>

        <h3>3.4 Technical Data</h3>
        <p>
          We collect IP addresses, browser user-agent strings, and device
          information for security purposes (rate limiting, abuse prevention,
          fraud detection, and session management).
        </p>

        <h2>4. Legal Basis for Processing</h2>
        <p>We process your Personal Data on the following legal bases:</p>
        <ul>
          <li>
            <strong className="text-zinc-200">Performance of a contract</strong>{" "}
            — to provide and maintain the Service, manage your account, and
            process transactions (GDPR Article 6(1)(b)).
          </li>
          <li>
            <strong className="text-zinc-200">Legitimate interest</strong> — for
            security, fraud prevention, service improvement, and aggregated
            analytics (GDPR Article 6(1)(f)).
          </li>
          <li>
            <strong className="text-zinc-200">Consent</strong> — for
            optional communications such as marketing emails. You may withdraw
            consent at any time (GDPR Article 6(1)(a)).
          </li>
          <li>
            <strong className="text-zinc-200">Legal obligation</strong> — where
            required by applicable law or regulation (GDPR Article 6(1)(c)).
          </li>
        </ul>

        <h2>5. Purpose of Processing</h2>
        <ul>
          <li>Provide, maintain, and improve the Service</li>
          <li>Manage user accounts and authentication</li>
          <li>Process billing and send transaction-related notifications</li>
          <li>Enforce rate limits, prevent abuse, and ensure platform security</li>
          <li>Send transactional emails (account verification, password resets, job notifications)</li>
          <li>Generate aggregated, anonymized analytics about Service usage</li>
          <li>Comply with legal and regulatory obligations</li>
        </ul>

        <h2>6. Data Retention</h2>
        <ul>
          <li>
            <strong className="text-zinc-200">Account data</strong> — retained
            for the duration of your active account.
          </li>
          <li>
            <strong className="text-zinc-200">Crawl job data &amp; analytics</strong>{" "}
            — retained for 90 days after job completion, unless earlier deletion
            is requested.
          </li>
          <li>
            <strong className="text-zinc-200">Billing records</strong> — retained
            for 10 years in accordance with the French Code de Commerce.
          </li>
          <li>
            <strong className="text-zinc-200">Litigation/compliance</strong> — up
            to 5 years post-termination of the contractual relationship.
          </li>
          <li>
            <strong className="text-zinc-200">Navigation/technical data</strong>{" "}
            — maximum 6 months.
          </li>
        </ul>
        <p>
          Upon account deletion, we remove your Personal Data within 30 days,
          except where retention is required by law. Anonymized data may be
          retained indefinitely.
        </p>

        <h2>7. Data Sharing and Recipients</h2>
        <p>We do not sell your Personal Data. We share data with:</p>
        <ul>
          <li>
            <strong className="text-zinc-200">Service providers</strong> —
            Stripe (payment processing), Resend (transactional emails),
            Heroku/cloud infrastructure providers (hosting). These processors
            act on our instructions and are bound by data processing agreements.
          </li>
          <li>
            <strong className="text-zinc-200">Affiliates</strong> — companies
            within the Meilisearch group, subject to this Privacy Policy.
          </li>
          <li>
            <strong className="text-zinc-200">Legal requirements</strong> — when
            required by law, court order, or governmental authority.
          </li>
        </ul>
        <p>
          A complete record of data disclosures is maintained. You may request
          access by contacting{" "}
          <a href="mailto:privacy@meilisearch.com" className="text-indigo-400 hover:text-indigo-300">
            privacy@meilisearch.com
          </a>.
        </p>

        <h2>8. International Transfers</h2>
        <p>
          Your data is primarily processed within the European Union. Where data
          is transferred outside the EU (e.g., to US-based service providers), we
          ensure appropriate safeguards are in place, including Standard
          Contractual Clauses approved by the European Commission.
        </p>

        <h2>9. Security</h2>
        <p>
          We implement industry-standard security measures including: passwords
          hashed with Argon2, API keys stored as SHA-256 hashes, JWT sessions
          with HttpOnly/Secure/SameSite cookies, TLS encryption in transit,
          role-based access control, and rate limiting. Despite our efforts, no
          method of transmission over the Internet or method of electronic
          storage is 100% secure.
        </p>

        <h2>10. Your Rights</h2>
        <p>
          Under the GDPR, French law no. 78-17 of January 6, 1978, and
          applicable data protection regulations, you have the right to:
        </p>
        <ul>
          <li>Access the Personal Data we hold about you</li>
          <li>Rectify inaccurate or incomplete data</li>
          <li>Request erasure of your data (&quot;right to be forgotten&quot;)</li>
          <li>Restrict or object to processing of your data</li>
          <li>Data portability — receive your data in a structured, machine-readable format</li>
          <li>Withdraw consent at any time (where processing is based on consent)</li>
          <li>Define guidelines regarding the fate of your data after death</li>
        </ul>
        <p>
          To exercise these rights, contact{" "}
          <a href="mailto:privacy@meilisearch.com" className="text-indigo-400 hover:text-indigo-300">
            privacy@meilisearch.com
          </a>.
          We will respond within 30 days. If a request is denied, we will
          provide an explanation within 30 days, including grounds for denial.
          You also have the right to file a complaint with the CNIL (Commission
          Nationale de l&apos;Informatique et des Libertes) or seek compensation
          through the courts.
        </p>

        <h2>11. Cookies</h2>
        <p>
          We use a single session cookie
          (<code className="text-zinc-300">scrapix_session</code>) for
          authentication. It is HttpOnly, Secure in production, and
          SameSite=Lax. We do not use tracking cookies, advertising cookies, or
          third-party analytics cookies. You may configure your browser to refuse
          cookies, but this will prevent you from using the Service.
        </p>

        <h2>12. Third-Party Links</h2>
        <p>
          The Service may contain links to third-party websites. We have no
          control over and assume no responsibility for the content or privacy
          practices of external sites. We encourage you to review their privacy
          policies independently.
        </p>

        <h2>13. Children</h2>
        <p>
          The Service is not directed to individuals under the age of 16. We do
          not knowingly collect Personal Data from children. If you believe we
          have collected data from a child, contact us immediately.
        </p>

        <h2>14. Changes to This Policy</h2>
        <p>
          We may update this Privacy Policy from time to time. We will provide
          at least 15 days&apos; advance notice before material changes take
          effect, except where changes are required by law or court order, in
          which case they may take effect immediately. Your continued use of the
          Service after changes constitutes acceptance of the updated policy.
        </p>

        <h2>15. Contact</h2>
        <p>
          Meilisearch SAS<br />
          52 boulevard de Sebastopol, 75003 Paris, France<br />
          Paris Trade and Companies Register: 844 156 364<br />
          Email:{" "}
          <a href="mailto:privacy@meilisearch.com" className="text-indigo-400 hover:text-indigo-300">
            privacy@meilisearch.com
          </a>
        </p>
      </div>
    </section>
  );
}
