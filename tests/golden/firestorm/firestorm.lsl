#define USE_SWITCHES
default {
    state_entry() {
        integer value = 1;
        switch (value) {
            case 1:
                llOwnerSay("one");
                break;
            default:
                break;
        }
    }
}
