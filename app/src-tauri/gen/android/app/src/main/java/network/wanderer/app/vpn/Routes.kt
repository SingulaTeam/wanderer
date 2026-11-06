package network.wanderer.app.vpn

/**
 * The IPv4 routes into the tunnel.
 *
 * All of the address space, or, when the local network stays reachable,
 * all of it but the local ranges. Computed rather than left to
 * `excludeRoute`, which needs Android 13.
 */
object Routes {
    /** Private, link-local and multicast IPv4: the local network. */
    private val LAN = listOf(
        "10.0.0.0" to 8,
        "172.16.0.0" to 12,
        "192.168.0.0" to 16,
        "169.254.0.0" to 16,
        "224.0.0.0" to 4,
    )

    fun ipv4(allowLan: Boolean): List<Pair<String, Int>> {
        if (!allowLan) return listOf("0.0.0.0" to 0)
        val excluded = LAN.map { (a, p) -> parse(a) to p }
        return complement(0L, 0, excluded).map { (a, p) -> format(a) to p }
    }

    /** The prefixes covering `base/prefix` minus every excluded one. */
    private fun complement(base: Long, prefix: Int, excluded: List<Pair<Long, Int>>): List<Pair<Long, Int>> {
        // Wholly excluded: nothing of it goes in.
        if (excluded.any { (a, p) -> p <= prefix && covers(a, p, base) }) return emptyList()
        // Nothing excluded within it: all of it goes in.
        if (excluded.none { (a, p) -> p > prefix && covers(base, prefix, a) }) return listOf(base to prefix)
        val half = 1L shl (31 - prefix)
        return complement(base, prefix + 1, excluded) + complement(base + half, prefix + 1, excluded)
    }

    /** Whether `a/p` contains the address `x`. */
    private fun covers(a: Long, p: Int, x: Long): Boolean =
        p == 0 || (a ushr (32 - p)) == (x ushr (32 - p))

    private fun parse(s: String): Long =
        s.split('.').fold(0L) { acc, part -> (acc shl 8) or part.toLong() }

    private fun format(a: Long): String =
        listOf(24, 16, 8, 0).joinToString(".") { ((a ushr it) and 0xff).toString() }
}
