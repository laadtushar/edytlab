/**
 * What the privacy page says about Google Analytics 4 on the website.
 *
 * Rendered by `app/privacy/page.tsx` only in a build that has a
 * measurement ID (`GA_MEASUREMENT_ID` in `lib/consent.ts`), so the page
 * never describes a service the site is not running. The banner and the
 * script are `google-analytics-consent.tsx`; the choice and the cookie
 * clean-up are `lib/consent.ts`. Change the copy here with them.
 */
export function GoogleAnalyticsDisclosure() {
  return (
    <>
      <h3 id="google-analytics">Google Analytics, only if you accept</h3>
      <p>
        This website can also use{" "}
        <a href="https://support.google.com/analytics/answer/6004245">
          Google Analytics 4
        </a>
        , a service of Google, to count visits. It is off until you choose.
        When you first visit, a banner asks you to accept or decline. Until
        you accept, your browser makes no request to Google for it and the
        site sets no Google Analytics cookie, and if you decline, nothing
        changes: the site works the same and nothing from Google is loaded.
        Your choice is kept in your browser&apos;s local storage on this
        site. The desktop app is not part of this and contains no Google
        Analytics: it still sends no telemetry.
      </p>
      <p>
        We use Google&apos;s Consent Mode with all four consent signals
        (<code>ad_storage</code>, <code>ad_user_data</code>,{" "}
        <code>ad_personalization</code> and <code>analytics_storage</code>)
        denied by default. Accepting turns on <code>analytics_storage</code>{" "}
        only. We do not use Google&apos;s advertising features, and the three
        advertising signals stay denied.
      </p>
      <p>
        <strong>What Google receives after you accept.</strong> The Google tag
        is loaded from <code>www.googletagmanager.com</code> and sets two
        first-party cookies, <code>_ga</code> and <code>_ga_&lt;ID&gt;</code>,
        which Google documents as lasting two years by default, to tell
        visitors and sessions apart. It then sends page views and basic
        engagement events to Google with the page address, the referrer,
        browser and device details (such as the browser, operating system,
        screen size and language) and an approximate location worked out from
        your IP address. Google states that Google Analytics does not log or
        store IP addresses, but processes them to provide the service. We send
        Google no names, email addresses or other details that identify you,
        and this website has no accounts. We see the results as aggregated
        reports in Google Analytics, and Google may process the data in
        countries other than your own, including the United States.
      </p>
      <p>
        <strong>How to withdraw.</strong> Choose &ldquo;Cookie settings&rdquo;
        in the footer of any page and press Decline. Measurement stops at
        once and the <code>_ga</code> cookies this site set are removed. The
        Google script that is already on the open page cannot be unloaded, so
        it is told to stop, and it is not loaded again when the page is next
        opened. You can also clear this site&apos;s data in your browser, or
        install Google&apos;s{" "}
        <a href="https://tools.google.com/dlpage/gaoptout/">
          opt-out browser add-on
        </a>
        .
      </p>
      <p>
        Google handles this data under its own terms. See Google&apos;s{" "}
        <a href="https://policies.google.com/privacy">Privacy Policy</a>,{" "}
        <a href="https://policies.google.com/technologies/partner-sites">
          How Google uses information from sites or apps that use its services
        </a>
        , and{" "}
        <a href="https://support.google.com/analytics/answer/6004245">
          Safeguarding your data
        </a>{" "}
        in Google Analytics, and the{" "}
        <a href="https://support.google.com/analytics/answer/1011397">
          data sharing settings
        </a>{" "}
        that govern what Google may do with it.
      </p>
    </>
  );
}
