integer gListener;
key gOwner;

default
{
    state_entry()
    {
        gOwner = llGetOwner();
        llListen(42, "", gOwner, "");
    }

    touch_start(integer total)
    {
        llDialog(gOwner, "Pick", ["Ok", "Cancel"], 42);
    }

    listen(integer channel, string name, key id, string message)
    {
        if (message == "Ok") llOwnerSay("accepted");
        llListenRemove(gListener);
    }
}
