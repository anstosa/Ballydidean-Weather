package farm.ballydidean.weather

internal object HostedAppIdentity {
    private val versionPattern = Regex("""\d+\.\d+(?:\.\d+)?(?:-[A-Za-z0-9][A-Za-z0-9.-]*)?""")
    private val identityPattern = Regex("""(?:^|\s)BallydideanWeather(?:Build)?/[^\s]+""")

    // expose public installed-build metadata without a javascript bridge
    fun userAgent(defaultUserAgent: String, versionName: String, versionCode: Int): String {
        require(versionName.length <= 64 && versionPattern.matches(versionName)) { "invalid app version" }
        require(versionCode in 1..2_100_000_000) { "invalid app build" }
        val original = defaultUserAgent.replace(identityPattern, "").trim()
        return "$original BallydideanWeather/$versionName BallydideanWeatherBuild/$versionCode"
    }
}
