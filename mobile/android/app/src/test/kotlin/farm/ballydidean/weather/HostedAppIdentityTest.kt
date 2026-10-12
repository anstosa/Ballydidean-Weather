package farm.ballydidean.weather

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class HostedAppIdentityTest {
    private val original = "Mozilla/5.0 (Linux; Android 16; Pixel; wv) AppleWebKit/537.36"

    // preserve every browser token while exposing the actual installed build
    @Test
    fun retainsDefaultIdentityAndRefreshesOnlyWeatherTokens() {
        val expected = "$original BallydideanWeather/0.1.2 BallydideanWeatherBuild/262852359"
        val actual = HostedAppIdentity.userAgent(original, "0.1.2", 262852359)
        assertEquals(expected, actual)
        assertTrue(actual.contains("; wv)"))
        assertEquals(expected, HostedAppIdentity.userAgent(actual, "0.1.2", 262852359))
        assertEquals(expected, HostedAppIdentity.userAgent(
            "$original BallydideanWeather/0.1.1 BallydideanWeatherBuild/1", "0.1.2", 262852359,
        ))
        assertTrue(HostedAppIdentity.userAgent(original, "0.1.2-debug", 1).contains("BallydideanWeather/0.1.2-debug"))
    }

    // reject malformed products rather than creating ambiguous user agents
    @Test(expected = IllegalArgumentException::class)
    fun rejectsUnsafeVersionNames() {
        HostedAppIdentity.userAgent(original, "0.1.2\nInjected/1", 1)
    }

    // preserve the play store's positive version-code boundary
    @Test(expected = IllegalArgumentException::class)
    fun rejectsInvalidBuildCodes() {
        HostedAppIdentity.userAgent(original, "0.1.2", 0)
    }
}
