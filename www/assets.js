// Fetches a static asset (image, sound) and returns an `ArrayBuffer`.
//
// Unlike a failed page load, a failed asset fetch is easy to miss: the page keeps working
// with the asset silently missing. Hence the retries and the loud logging. Non-OK HTTP
// status is treated as a failure (`fetch` itself resolves fine on 404/500: the server did
// reply, just not with the asset).

const FETCH_RETRY_DELAYS_MS = [500, 2000];

export async function fetch_asset(url) {
  for (let attempt = 0; ; ++attempt) {
    try {
      const response = await fetch(url);
      if (!response.ok) {
        throw new Error(`HTTP status ${response.status}`);
      }
      return await response.arrayBuffer();
    } catch (error) {
      if (attempt >= FETCH_RETRY_DELAYS_MS.length) {
        console.error(`Failed to fetch ${url} (attempts: ${attempt + 1}):`, error);
        throw error;
      }
      console.warn(`Failed to fetch ${url}, will retry:`, error);
      await new Promise((resolve) => setTimeout(resolve, FETCH_RETRY_DELAYS_MS[attempt]));
    }
  }
}
